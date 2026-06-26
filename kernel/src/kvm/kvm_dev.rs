use alloc::vec::Vec;

use super::vm::{
    KvmError, VmConfig, create_vm, init_vm_manager,
};

pub const KVM_API_VERSION: u32 = 12;
pub const KVM_RUN_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KvmDeviceOps {
    GetApiVersion,
    CreateVm,
    CreateVcpu,
    Run,
    SetUserMemoryRegion,
    GetVcpuMmapSize,
}

#[derive(Debug, Clone)]
pub struct KvmDevice {
    pub version: u32,
    pub vm_fds: Vec<u32>,
}

impl KvmDevice {
    pub fn new() -> Self {
        Self {
            version: KVM_API_VERSION,
            vm_fds: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct KvmRun {
    pub exit_reason: u32,
    pub io_port: u16,
    pub io_direction: u8,
    pub io_size: u8,
    pub io_count: u16,
    pub io_data_offset: u16,
    pub padding: [u8; 4096 - 12],
}

impl KvmRun {
    pub fn new() -> Self {
        Self {
            exit_reason: 0,
            io_port: 0,
            io_direction: 0,
            io_size: 0,
            io_count: 0,
            io_data_offset: 0,
            padding: [0u8; 4096 - 12],
        }
    }
}

pub fn kvm_ioctl(op: u32, _arg: u64) -> Result<u64, KvmError> {
    let device_op = match op {
        0x00 => KvmDeviceOps::GetApiVersion,
        0x01 => KvmDeviceOps::CreateVm,
        0x44 => KvmDeviceOps::Run,
        0x46 => KvmDeviceOps::GetVcpuMmapSize,
        _ => return Err(KvmError::InvalidConfig),
    };

    match device_op {
        KvmDeviceOps::GetApiVersion => Ok(KVM_API_VERSION as u64),
        KvmDeviceOps::CreateVm => {
            let config = VmConfig {
                memory_size: 1024 * 1024,
                num_cpus: 1,
                kernel_entry: 0x1000_0000,
                initrd_start: 0x2000_0000,
                initrd_size: 0x1000,
            };
            let id = create_vm(&config)?;
            Ok(id as u64)
        }
        KvmDeviceOps::Run => Ok(0),
        KvmDeviceOps::GetVcpuMmapSize => Ok(KVM_RUN_SIZE as u64),
        _ => Err(KvmError::InvalidConfig),
    }
}

pub fn kvm_init() {
    init_vm_manager();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial::acquire;

    #[test]
    fn kvm_api_version() {
        let _guard = acquire();
        assert_eq!(KVM_API_VERSION, 12);
    }

    #[test]
    fn kvm_run_size() {
        let _guard = acquire();
        assert_eq!(KVM_RUN_SIZE, 4096);
    }

    #[test]
    fn kvm_get_api_version_ioctl() {
        let _guard = acquire();
        let result = kvm_ioctl(0x00, 0).unwrap();
        assert_eq!(result, 12);
    }

    #[test]
    fn kvm_create_vm_ioctl() {
        let _guard = acquire();
        super::super::vm::reset_vm_manager();
        kvm_init();

        let result = kvm_ioctl(0x01, 0).unwrap();
        assert!(result > 0);
    }

    #[test]
    fn kvm_get_vcpu_mmap_size() {
        let _guard = acquire();
        let result = kvm_ioctl(0x46, 0).unwrap();
        assert_eq!(result, KVM_RUN_SIZE as u64);
    }

    #[test]
    fn kvm_invalid_op() {
        let _guard = acquire();
        let result = kvm_ioctl(0xFF, 0);
        assert!(result.is_err());
    }

    #[test]
    fn kvm_run_new() {
        let _guard = acquire();
        let run = KvmRun::new();
        assert_eq!(run.exit_reason, 0);
        assert_eq!(run.io_port, 0);
    }

    #[test]
    fn kvm_device_new() {
        let _guard = acquire();
        let dev = KvmDevice::new();
        assert_eq!(dev.version, KVM_API_VERSION);
        assert!(dev.vm_fds.is_empty());
    }
}
