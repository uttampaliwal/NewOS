#![no_main]
#![no_std]

use core::arch::asm;
use core::fmt::{self, Write};
use core::panic::PanicInfo;

use newos_kernel::kernel_info;
use uefi::prelude::*;

const COM1_BASE: u16 = 0x3F8;
const QEMU_DEBUG_EXIT_PORT: u16 = 0xF4;

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

    let info = kernel_info();
    serial_println!("NewOS Phase 1 UEFI bring-up");
    serial_println!("project: {}", info.project_name);
    serial_println!("abi: {}", info.abi_version);
    serial_println!("status: boot path reached");

    qemu_exit_success();
}

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    serial::init();
    serial::print(format_args!("panic: {}\n", info));
    qemu_exit_failure();
}

fn qemu_exit_success() -> ! {
    qemu_exit(0x10);
}

fn qemu_exit_failure() -> ! {
    qemu_exit(0x11);
}

fn qemu_exit(code: u32) -> ! {
    unsafe {
        out32(QEMU_DEBUG_EXIT_PORT, code);
    }

    loop {
        halt();
    }
}

fn halt() {
    unsafe {
        asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

unsafe fn out8(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
}

unsafe fn out32(port: u16, value: u32) {
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack, preserves_flags));
    }
}

unsafe fn in8(port: u16) -> u8 {
    let value: u8;

    unsafe {
        asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
    }

    value
}

mod serial {
    use super::*;

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
}
