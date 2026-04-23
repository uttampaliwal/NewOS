use core::fmt::Write;

use newos_abi::boot::{
    BootEnvironment, BootInfo, BootLoaderKind, BootOutcome,
};

use crate::kernel_info;
use crate::memory::{FrameAllocator, MemorySummary};
use crate::serial::{self, SerialWriter};
use alloc::vec::Vec;
use x86_64::VirtAddr;

pub fn early_boot(boot_info: &BootInfo) -> BootOutcome {
    serial::init();

    let mut writer = SerialWriter;
    let kernel = kernel_info();
    let memory_summary = MemorySummary::from_boot_info(boot_info);
    let mut frame_allocator = FrameAllocator::new(boot_info);

    let first_frame = frame_allocator.allocate_physical_frame();
    let second_frame = frame_allocator.allocate_physical_frame();
    let third_frame = frame_allocator.allocate_physical_frame();

    let _ = writeln!(writer, "NewOS freestanding kernel reached");
    let _ = writeln!(writer, "project: {}", kernel.project_name);
    let _ = writeln!(writer, "kernel abi: {}", kernel.abi_version);
    let _ = writeln!(writer, "boot abi: {}", boot_info.abi_version);
    let _ = writeln!(writer, "environment: {}", describe_environment(boot_info.environment));
    let _ = writeln!(writer, "loader: {}", describe_loader(boot_info.loader));
    let _ = writeln!(writer, "boot services exited: {}", boot_info.boot_services_exited());
    let _ = writeln!(writer, "kernel image base: 0x{:016x}", boot_info.kernel_image_base);
    let _ = writeln!(writer, "kernel image size: {} bytes", boot_info.kernel_image_size);
    let _ = writeln!(writer, "memory map entries: {}", memory_summary.descriptor_count);
    let _ = writeln!(
        writer,
        "conventional regions: {}",
        memory_summary.conventional_region_count
    );
    let _ = writeln!(
        writer,
        "conventional memory pages: {}",
        memory_summary.conventional_page_count
    );
    let _ = writeln!(
        writer,
        "largest conventional region: {} pages",
        memory_summary.largest_conventional_region_pages
    );
    let _ = writeln!(
        writer,
        "frame allocator sample: {:#018x}, {:#018x}, {:#018x}",
        first_frame.map(|f| f.start_address).unwrap_or(0),
        second_frame.map(|f| f.start_address).unwrap_or(0),
        third_frame.map(|f| f.start_address).unwrap_or(0),
    );
    let _ = writeln!(writer, "status: physical frame allocator reached");

    // Initialize paging with an offset of 0 (since we are currently on UEFI identity mapping)
    let mut mapper = unsafe { crate::memory::paging::init(VirtAddr::new(0), &mut frame_allocator) };
    
    // Initialize the kernel heap
    crate::memory::heap::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

    let mut heap_test = Vec::new();
    for i in 0..5 {
        heap_test.push(i);
    }
    let _ = writeln!(writer, "status: heap initialized, vec test: {:?}", heap_test);

    BootOutcome::ExitSuccess
}

const fn describe_environment(environment: BootEnvironment) -> &'static str {
    match environment {
        BootEnvironment::Unknown => "unknown",
        BootEnvironment::Uefi => "uefi",
    }
}

const fn describe_loader(loader: BootLoaderKind) -> &'static str {
    match loader {
        BootLoaderKind::Unknown => "unknown",
        BootLoaderKind::UefiLoader => "uefi-loader",
    }
}
