use core::fmt::Write;

use newos_abi::boot::{
    BootEnvironment, BootInfo, BootLoaderKind, BootOutcome, MEMORY_TYPE_CONVENTIONAL,
};

use crate::kernel_info;
use crate::serial::{self, SerialWriter};

pub fn early_boot(boot_info: &BootInfo) -> BootOutcome {
    serial::init();

    let mut writer = SerialWriter;
    let kernel = kernel_info();

    let conventional_pages = boot_info
        .memory_map
        .entry_count()
        .checked_sub(0)
        .map(|_| {
            let mut total = 0u64;
            for index in 0..boot_info.memory_map.entry_count() {
                if let Some(descriptor) = boot_info.memory_map.get(index) {
                    if descriptor.ty == MEMORY_TYPE_CONVENTIONAL {
                        total += descriptor.page_count;
                    }
                }
            }
            total
        })
        .unwrap_or(0);

    let _ = writeln!(writer, "NewOS freestanding kernel reached");
    let _ = writeln!(writer, "project: {}", kernel.project_name);
    let _ = writeln!(writer, "kernel abi: {}", kernel.abi_version);
    let _ = writeln!(writer, "boot abi: {}", boot_info.abi_version);
    let _ = writeln!(writer, "environment: {}", describe_environment(boot_info.environment));
    let _ = writeln!(writer, "loader: {}", describe_loader(boot_info.loader));
    let _ = writeln!(writer, "boot services exited: {}", boot_info.boot_services_exited());
    let _ = writeln!(writer, "kernel image base: 0x{:016x}", boot_info.kernel_image_base);
    let _ = writeln!(writer, "kernel image size: {} bytes", boot_info.kernel_image_size);
    let _ = writeln!(writer, "memory map entries: {}", boot_info.memory_map.entry_count());
    let _ = writeln!(writer, "conventional memory pages: {}", conventional_pages);
    let _ = writeln!(writer, "status: freestanding kernel handoff reached");

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
