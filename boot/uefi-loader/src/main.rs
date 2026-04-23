#![no_main]
#![no_std]

extern crate alloc;

mod elf;

use core::arch::asm;
use core::fmt::{self, Write};
use core::panic::PanicInfo;
use core::ptr::NonNull;

use elf::LoadedKernel;
use newos_abi::boot::{BOOT_FLAG_BOOT_SERVICES_EXITED, BootInfo, BootMemoryMap};
use newos_abi::version::ABI_VERSION;
use uefi::boot::{self, AllocateType};
use uefi::fs::FileSystem;
use uefi::mem::memory_map::{MemoryMap, MemoryType};
use uefi::prelude::*;
use uefi::{Status, cstr16};

const QEMU_DEBUG_EXIT_PORT: u16 = 0xF4;
const KERNEL_IMAGE_PATH: &uefi::CStr16 = cstr16!(r"\newos\kernel.elf");

macro_rules! serial_println {
    () => {
        $crate::serial::print(format_args!("\n"))
    };
    ($format:literal $(, $arg:expr)*) => {
        $crate::serial::print(format_args!(concat!($format, "\n") $(, $arg)*))
    };
}

#[entry]
fn main() -> Status {
    serial::init();
    let _ = boot::set_watchdog_timer(0, 0, None);

    serial_println!("NewOS UEFI loader started");
    let boot_info_ptr = allocate_boot_info();

    let loaded_kernel = {
        let image_fs = boot::get_image_file_system(boot::image_handle()).expect("image filesystem should be available");
        let mut file_system = FileSystem::new(image_fs);
        let kernel_bytes = file_system.read(KERNEL_IMAGE_PATH).expect("kernel image should be readable");
        let loaded_kernel = elf::load_kernel(&kernel_bytes).expect("kernel ELF should load successfully");

        serial_println!("kernel file loaded");
        serial_println!("entry: 0x{:016x}", loaded_kernel.entry_point);
        serial_println!("base: 0x{:016x}", loaded_kernel.image_base);
        serial_println!("size: {} bytes", loaded_kernel.image_size);

        loaded_kernel
    };

    unsafe {
        (*boot_info_ptr.as_ptr()) = boot_info_template(loaded_kernel);
    }

    serial_println!("exiting boot services");
    let memory_map = unsafe { boot::exit_boot_services(Some(MemoryType::LOADER_DATA)) };

    unsafe {
        let boot_info = &mut *boot_info_ptr.as_ptr();
        boot_info.flags |= BOOT_FLAG_BOOT_SERVICES_EXITED;
        boot_info.memory_map = BootMemoryMap {
            descriptors: memory_map.buffer().as_ptr().cast(),
            map_size: memory_map.meta().map_size,
            desc_size: memory_map.meta().desc_size,
            desc_version: memory_map.meta().desc_version,
        };

        jump_to_kernel(loaded_kernel.entry_point, boot_info as *const BootInfo)
    }
}

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    serial::init();
    serial::print(format_args!("panic: {}\n", info));
    qemu_exit_failure();
}

fn boot_info_template(loaded_kernel: LoadedKernel) -> BootInfo {
    let mut boot_info = BootInfo::uefi(ABI_VERSION);
    boot_info.kernel_image_base = loaded_kernel.image_base;
    boot_info.kernel_image_size = loaded_kernel.image_size;
    boot_info
}

fn allocate_boot_info() -> NonNull<BootInfo> {
    let page = boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        1,
    )
    .expect("boot info page allocation should succeed");

    page.cast()
}

unsafe fn jump_to_kernel(entry_point: u64, boot_info: *const BootInfo) -> ! {
    type KernelEntry = extern "sysv64" fn(*const BootInfo) -> !;
    let entry: KernelEntry = unsafe { core::mem::transmute(entry_point as usize) };
    entry(boot_info)
}

fn qemu_exit_failure() -> ! {
    qemu_exit(0x11);
}

fn qemu_exit(code: u32) -> ! {
    unsafe {
        asm!("out dx, eax", in("dx") QEMU_DEBUG_EXIT_PORT, in("eax") code, options(nomem, nostack, preserves_flags));
    }

    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

mod serial {
    use super::*;

    const COM1_BASE: u16 = 0x3F8;

    pub fn init() {
        unsafe {
            out8(COM1_BASE + 1, 0x00);
            out8(COM1_BASE + 3, 0x80);
            out8(COM1_BASE, 0x03);
            out8(COM1_BASE + 1, 0x00);
            out8(COM1_BASE + 3, 0x03);
            out8(COM1_BASE + 2, 0xC7);
            out8(COM1_BASE + 4, 0x0B);
        }
    }

    pub fn print(args: fmt::Arguments<'_>) {
        let mut writer = SerialWriter;
        let _ = writer.write_fmt(args);
    }

    struct SerialWriter;

    impl SerialWriter {
        fn write_byte(&mut self, byte: u8) {
            unsafe {
                while (in8(COM1_BASE + 5) & 0x20) == 0 {}
                out8(COM1_BASE, byte);
            }
        }
    }

    impl Write for SerialWriter {
        fn write_str(&mut self, value: &str) -> fmt::Result {
            for byte in value.bytes() {
                match byte {
                    b'\n' => {
                        self.write_byte(b'\r');
                        self.write_byte(b'\n');
                    }
                    _ => self.write_byte(byte),
                }
            }

            Ok(())
        }
    }

    unsafe fn out8(port: u16, value: u8) {
        unsafe {
            asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
        }
    }

    unsafe fn in8(port: u16) -> u8 {
        let value: u8;

        unsafe {
            asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
        }

        value
    }
}
