use core::fmt::{self, Write};

use newos_abi::boot::{BootEnvironment, BootInfo, BootLoaderKind, BootOutcome};

use crate::kernel_info;

pub trait BootConsole {
    fn write_str(&mut self, value: &str);
}

pub fn early_boot<C: BootConsole>(console: &mut C, boot_info: &BootInfo) -> BootOutcome {
    let mut writer = ConsoleWriter { console };
    let kernel = kernel_info();

    let _ = writeln!(writer, "NewOS kernel stage reached");
    let _ = writeln!(writer, "project: {}", kernel.project_name);
    let _ = writeln!(writer, "kernel abi: {}", kernel.abi_version);
    let _ = writeln!(writer, "boot abi: {}", boot_info.abi_version);
    let _ = writeln!(writer, "environment: {}", describe_environment(boot_info.environment));
    let _ = writeln!(writer, "loader: {}", describe_loader(boot_info.loader));
    let _ = writeln!(writer, "status: kernel handoff path reached");

    BootOutcome::ExitSuccess
}

struct ConsoleWriter<'a, C> {
    console: &'a mut C,
}

impl<C: BootConsole> Write for ConsoleWriter<'_, C> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.console.write_str(value);
        Ok(())
    }
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
