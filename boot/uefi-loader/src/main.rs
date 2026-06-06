#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod elf;

use core::arch::asm;
use core::panic::PanicInfo;

use elf::LoadedKernel;
use turnix_abi::boot::{BOOT_FLAG_BOOT_SERVICES_EXITED, BootInfo, BootMemoryMap};
use turnix_abi::version::ABI_VERSION;
use uefi::boot::{self, AllocateType};
use uefi::fs::FileSystem;
use uefi::mem::memory_map::{MemoryMap, MemoryType};
use uefi::prelude::*;
use uefi::{Status, cstr16};
use x86_64::VirtAddr;
use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::structures::paging::{
    Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size2MiB, Size4KiB,
};

const QEMU_DEBUG_EXIT_PORT: u16 = 0xF4;
const KERNEL_IMAGE_PATH: &uefi::CStr16 = cstr16!(r"\turnix\kernel.elf");
const RAMDISK_IMAGE_PATH: &uefi::CStr16 = cstr16!(r"\turnix\initramfs.img");
const PHYSICAL_MEMORY_OFFSET: u64 = 0xffff_8000_0000_0000;
const RAMDISK_VIRTUAL_BASE: u64 = 0xffff_9000_0000_0000;
const KERNEL_VIRTUAL_BASE: u64 = 0xffff_ffff_8000_0000;

use turnix_serial::{self as serial, println as serial_println};

#[cfg(not(test))]
#[entry]
fn main() -> Status {
    serial::init();
    let _ = boot::set_watchdog_timer(0, 0, None);

    serial_println!("turnix UEFI loader");

    // Compute KASLR offset before loading the kernel (needed for RELA fixups)
    let kaslr_offset = generate_kaslr_offset();

    // 1. Load Kernel ELF and Ramdisk
    let (loaded_kernel, ramdisk_phys, ramdisk_size) = {
        let image_fs = boot::get_image_file_system(boot::image_handle())
            .expect("image filesystem should be available");
        let mut file_system = FileSystem::new(image_fs);

        let kernel_bytes = file_system
            .read(KERNEL_IMAGE_PATH)
            .expect("kernel image should be readable");
        let loaded_kernel = elf::load_kernel(&kernel_bytes, kaslr_offset)
            .expect("kernel ELF should load successfully");
        serial_println!("kernel loaded: entry=0x{:016x}", loaded_kernel.entry_point);

        let ramdisk_bytes = file_system.read(RAMDISK_IMAGE_PATH).unwrap_or_else(|_| {
            serial_println!("WARNING: ramdisk not found at {:?}", RAMDISK_IMAGE_PATH);
            alloc::vec::Vec::new()
        });

        let mut phys_addr = 0;
        let mut size = 0;

        if !ramdisk_bytes.is_empty() {
            size = ramdisk_bytes.len() as u64;
            let pages = size.div_ceil(4096);
            let ramdisk_alloc = boot::allocate_pages(
                AllocateType::AnyPages,
                MemoryType::LOADER_DATA,
                pages as usize,
            )
            .expect("failed to allocate ramdisk pages");
            phys_addr = ramdisk_alloc.as_ptr() as u64;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    ramdisk_bytes.as_ptr(),
                    phys_addr as *mut u8,
                    ramdisk_bytes.len(),
                );
            }
            serial_println!(
                "ramdisk loaded: phys=0x{:016x}, size={} bytes",
                phys_addr,
                size
            );
        }

        (loaded_kernel, phys_addr, size)
    };

    // 2. Allocate persistent data while Boot Services are active
    let boot_info_page = boot::allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, 1)
        .expect("failed to allocate BootInfo");
    let boot_info_ptr = boot_info_page.as_ptr() as *mut BootInfo;

    let scratchpad_pages = 256; // Increased for more mappings
    let scratchpad_ptr = boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        scratchpad_pages,
    )
    .expect("failed to allocate scratchpad");
    let scratchpad_phys = scratchpad_ptr.as_ptr() as u64;

    let map_meta = boot::memory_map(MemoryType::LOADER_DATA)
        .expect("failed to get map meta")
        .meta();
    let map_buffer_ptr = boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        map_meta.map_size.div_ceil(4096) + 1,
    )
    .expect("failed to allocate map buffer");

    // 3. Build the NEW PML4 while we still have Boot Services
    let new_pml4_phys = scratchpad_phys;
    let new_pml4 = unsafe { &mut *(new_pml4_phys as *mut PageTable) };

    unsafe {
        core::ptr::write_bytes(new_pml4 as *mut PageTable, 0, 1);
        setup_mappings(
            new_pml4,
            scratchpad_phys,
            loaded_kernel,
            ramdisk_phys,
            ramdisk_size,
            kaslr_offset,
        );
    }

    // 4. Get GOP info before exiting boot services
    let gop = boot::get_handle_for_protocol::<uefi::proto::console::gop::GraphicsOutput>()
        .and_then(|h| boot::open_protocol_exclusive::<uefi::proto::console::gop::GraphicsOutput>(h))
        .expect("GOP should be available");

    let mut gop = gop;
    let fb_info = gop.current_mode_info();
    let fb_addr = gop.frame_buffer().as_mut_ptr() as u64;
    let fb_size = gop.frame_buffer().size() as u64;

    // 5. Exit Boot Services
    serial_println!("exiting boot services");

    // Find ACPI RSDP before exiting boot services
    let rsdp_addr = 0;
    // TODO: Fix RSDP detection for uefi 0.37 API
    // For now, RSDP detection is disabled - ACPI will be unavailable
    serial_println!("RSDP detection disabled for now");

    let memory_map = unsafe { boot::exit_boot_services(Some(MemoryType::LOADER_DATA)) };

    // 6. Populate Persistent Data
    unsafe {
        core::ptr::copy_nonoverlapping(
            memory_map.buffer().as_ptr(),
            map_buffer_ptr.as_ptr(),
            memory_map.meta().map_size,
        );

        let boot_info = &mut *boot_info_ptr;
        *boot_info = BootInfo::uefi(ABI_VERSION);
        boot_info.flags |= BOOT_FLAG_BOOT_SERVICES_EXITED;
        boot_info.kernel_image_base = KERNEL_VIRTUAL_BASE + kaslr_offset;
        boot_info.kernel_image_size = loaded_kernel.image_size;
        boot_info.kaslr_offset = kaslr_offset;
        boot_info.physical_memory_offset = PHYSICAL_MEMORY_OFFSET;
        boot_info.ramdisk_addr = if ramdisk_size > 0 {
            RAMDISK_VIRTUAL_BASE
        } else {
            0
        };
        boot_info.ramdisk_size = ramdisk_size;
        boot_info.memory_map = BootMemoryMap {
            descriptors: map_buffer_ptr.as_ptr().cast(),
            map_size: memory_map.meta().map_size,
            desc_size: memory_map.meta().desc_size,
            desc_version: memory_map.meta().desc_version as u32,
        };
        boot_info.framebuffer = turnix_abi::boot::BootFramebuffer {
            addr: fb_addr,
            size: fb_size,
            width: fb_info.resolution().0 as u32,
            height: fb_info.resolution().1 as u32,
            pitch: fb_info.stride() as u32,
            format: match fb_info.pixel_format() {
                uefi::proto::console::gop::PixelFormat::Bgr => 0,
                uefi::proto::console::gop::PixelFormat::Rgb => 1,
                _ => 0,
            },
        };
        boot_info.rsdp_addr = rsdp_addr;

        // 7. Final Transition
        serial_println!("Switching CR3...");
        Cr3::write(
            PhysFrame::containing_address(x86_64::PhysAddr::new(new_pml4_phys)),
            Cr3Flags::empty(),
        );

        serial_println!("Jumping to kernel...");
        jump_to_kernel(
            loaded_kernel.entry_point + kaslr_offset,
            boot_info as *const BootInfo,
        )
    }
}

#[cfg(test)]
fn main() {}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    serial::init();
    serial::print!("panic: {}\n", info);
    qemu_exit_failure();
}

unsafe fn setup_mappings(
    pml4: &mut PageTable,
    scratchpad_phys: u64,
    kernel: LoadedKernel,
    ramdisk_phys: u64,
    ramdisk_size: u64,
    kaslr_offset: u64,
) {
    let (old_pml4_frame, _) = Cr3::read();
    let old_pml4 = unsafe { &*(old_pml4_frame.start_address().as_u64() as *const PageTable) };

    // 1. Full clone of UEFI identity mappings (0-256 GB range)
    for i in 0..256 {
        pml4[i] = old_pml4[i].clone();
    }

    let mut mapper = unsafe { OffsetPageTable::new(pml4, VirtAddr::new(0)) };

    let mut next_scratch_phys = scratchpad_phys + 4096;
    let mut alloc = |size: usize| {
        let addr = next_scratch_phys;
        next_scratch_phys += size as u64;
        PhysFrame::<Size4KiB>::containing_address(x86_64::PhysAddr::new(addr))
    };

    struct ScratchAllocator<'a, F: FnMut(usize) -> PhysFrame<Size4KiB>>(&'a mut F);
    unsafe impl<'a, F: FnMut(usize) -> PhysFrame<Size4KiB>>
        x86_64::structures::paging::FrameAllocator<Size4KiB> for ScratchAllocator<'a, F>
    {
        fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
            Some((self.0)(4096))
        }
    }
    let mut scratch_alloc = ScratchAllocator(&mut alloc);

    // 2. Map Kernel to Higher-Half using 4KB pages
    let kernel_load_addr = KERNEL_VIRTUAL_BASE + kaslr_offset;
    let page_count = kernel.image_size.div_ceil(4096);
    for i in 0..page_count {
        let page: Page<Size4KiB> =
            Page::containing_address(VirtAddr::new(kernel_load_addr + i * 4096));
        let frame =
            PhysFrame::containing_address(x86_64::PhysAddr::new(kernel.physical_base + i * 4096));
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper
                .map_to(page, frame, flags, &mut scratch_alloc)
                .expect("failed to map kernel")
                .ignore();
        }
    }

    // 3. Map Ramdisk to Higher-Half (0xffff900000000000) using 4KB pages
    if ramdisk_size > 0 {
        let ramdisk_page_count = ramdisk_size.div_ceil(4096);
        for i in 0..ramdisk_page_count {
            let page: Page<Size4KiB> =
                Page::containing_address(VirtAddr::new(RAMDISK_VIRTUAL_BASE + i * 4096));
            let frame =
                PhysFrame::containing_address(x86_64::PhysAddr::new(ramdisk_phys + i * 4096));
            let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
            unsafe {
                mapper
                    .map_to(page, frame, flags, &mut scratch_alloc)
                    .expect("failed to map ramdisk")
                    .ignore();
            }
        }
    }

    // 4. Map first 4GB of physical RAM to Higher-Half Offset (0xffff800000000000)
    // USING 2MB HUGE PAGES for efficiency.
    for i in 0..(4096 / 2) {
        let addr = PHYSICAL_MEMORY_OFFSET + (i as u64) * 2 * 1024 * 1024;
        let page: x86_64::structures::paging::Page<Size2MiB> =
            x86_64::structures::paging::Page::containing_address(VirtAddr::new(addr));
        let frame = PhysFrame::<Size2MiB>::containing_address(x86_64::PhysAddr::new(
            (i as u64) * 2 * 1024 * 1024,
        ));
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

        unsafe {
            // map_to for 2MiB pages requires a different mapper trait or manual entry
            // For simplicity and compatibility, we'll use manual entry setting for the 2MB pages
            let pml4_idx = page.start_address().p4_index();
            let pdpt_idx = page.start_address().p3_index();
            let pd_idx = page.start_address().p2_index();

            // Ensure PDPT exists
            if pml4[pml4_idx].is_unused() {
                let new_frame = scratch_alloc.0(4096);
                pml4[pml4_idx].set_frame(
                    new_frame,
                    PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                );
                core::ptr::write_bytes(new_frame.start_address().as_u64() as *mut u8, 0, 4096);
            }
            let pdpt =
                &mut *(pml4[pml4_idx].frame().unwrap().start_address().as_u64() as *mut PageTable);

            // Ensure PD exists
            if pdpt[pdpt_idx].is_unused() {
                let new_frame = scratch_alloc.0(4096);
                pdpt[pdpt_idx].set_frame(
                    new_frame,
                    PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                );
                core::ptr::write_bytes(new_frame.start_address().as_u64() as *mut u8, 0, 4096);
            }
            let pd =
                &mut *(pdpt[pdpt_idx].frame().unwrap().start_address().as_u64() as *mut PageTable);

            // Set 2MB entry with HUGE_PAGE flag
            pd[pd_idx].set_addr(frame.start_address(), flags | PageTableFlags::HUGE_PAGE);
        }
    }
}

type KernelEntry = unsafe extern "sysv64" fn(*const BootInfo) -> !;

unsafe fn jump_to_kernel(entry_point: u64, boot_info: *const BootInfo) -> ! {
    let entry: KernelEntry = unsafe { core::mem::transmute(entry_point as usize) };
    unsafe { entry(boot_info) }
}

/// Generate a page-aligned KASLR offset with at least 9 bits of entropy
/// derived from RDRAND (req 15.2).
const KASLR_PAGE_ENTROPY: u64 = 9;
const KASLR_RANGE_PAGES: u64 = 1 << KASLR_PAGE_ENTROPY;

fn generate_kaslr_offset() -> u64 {
    // Attempt RDRAND; fall back to 0 if the instruction is not available
    // (should not happen on any real x86_64 UEFI system).
    let mut val: u64 = 0;
    let ok = unsafe { core::arch::x86_64::_rdrand64_step(&mut val) == 1 };
    if ok {
        let page_offset = val % KASLR_RANGE_PAGES;
        page_offset * 4096
    } else {
        0
    }
}

fn qemu_exit_failure() -> ! {
    unsafe {
        asm!("out dx, eax", in("dx") QEMU_DEBUG_EXIT_PORT, in("eax") 0x11u32, options(nomem, nostack, preserves_flags));
    }
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}
