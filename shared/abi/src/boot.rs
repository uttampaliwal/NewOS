#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootEnvironment {
    Unknown = 0,
    Uefi = 1,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootLoaderKind {
    Unknown = 0,
    UefiLoader = 1,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootOutcome {
    ExitSuccess = 0,
    ExitFailure = 1,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootInfo {
    pub abi_version: u32,
    pub environment: BootEnvironment,
    pub loader: BootLoaderKind,
    pub flags: u32,
}

impl BootInfo {
    pub const fn uefi(abi_version: u32) -> Self {
        Self {
            abi_version,
            environment: BootEnvironment::Uefi,
            loader: BootLoaderKind::UefiLoader,
            flags: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uefi_boot_info_uses_expected_defaults() {
        let boot_info = BootInfo::uefi(1);

        assert_eq!(boot_info.abi_version, 1);
        assert_eq!(boot_info.environment, BootEnvironment::Uefi);
        assert_eq!(boot_info.loader, BootLoaderKind::UefiLoader);
        assert_eq!(boot_info.flags, 0);
    }
}
