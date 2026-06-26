use super::vm::{Vcpu, VirtualMachine};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Hlt,
    IoInstruction,
    Cpuid,
    MsrAccess,
    EptViolation,
    ExternalInterrupt,
    TripleFault,
    Unknown(u32),
}

#[derive(Debug, Clone)]
pub struct ExitInfo {
    pub reason: ExitReason,
    pub qualification: u64,
    pub guest_rip: u64,
    pub instruction_len: u32,
}

pub fn decode_exit(reason_code: u32, _qualification: u64) -> ExitReason {
    match reason_code {
        0x0C => ExitReason::Hlt,
        0x1E | 0x30 => ExitReason::IoInstruction,
        0x0A => ExitReason::Cpuid,
        0x1F | 0x31 => ExitReason::MsrAccess,
        0x32 | 0x33 => ExitReason::EptViolation,
        0x28 => ExitReason::ExternalInterrupt,
        0x02 => ExitReason::TripleFault,
        other => ExitReason::Unknown(other),
    }
}

pub fn handle_exit(vm: &mut VirtualMachine, vcpu_id: u32, exit: ExitInfo) -> bool {
    let vcpu = match vm.vcpus.iter_mut().find(|v| v.id == vcpu_id) {
        Some(v) => v,
        None => return false,
    };

    match exit.reason {
        ExitReason::Hlt => {
            vm.state = super::vm::VmState::Halted;
            false
        }
        ExitReason::TripleFault => {
            vm.state = super::vm::VmState::Error;
            false
        }
        ExitReason::IoInstruction => {
            handle_io(vcpu, 0, false, &[]);
            vcpu.rip += exit.instruction_len as u64;
            true
        }
        ExitReason::Cpuid => {
            let leaf = vcpu.regs[0] as u32;
            let (eax, ebx, ecx, edx) = handle_cpuid(leaf);
            vcpu.regs[0] = eax as u64;
            vcpu.regs[1] = ebx as u64;
            vcpu.regs[2] = ecx as u64;
            vcpu.regs[3] = edx as u64;
            vcpu.rip += exit.instruction_len as u64;
            true
        }
        ExitReason::MsrAccess => {
            vcpu.rip += exit.instruction_len as u64;
            true
        }
        ExitReason::EptViolation => {
            vm.state = super::vm::VmState::Error;
            false
        }
        ExitReason::ExternalInterrupt => {
            vcpu.rip += exit.instruction_len as u64;
            true
        }
        ExitReason::Unknown(_) => {
            vcpu.rip += exit.instruction_len as u64;
            true
        }
    }
}

pub fn handle_cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    match leaf {
        0 => (0x0000_000D, 0x756E_6547, 0x6C65_746E, 0x4965_6E69),
        1 => (0x0003_06C3, 0x0080_0800, 0x7FFA_320B, 0xBFEB_FBFF),
        _ => (0, 0, 0, 0),
    }
}

pub fn handle_io(vcpu: &mut Vcpu, port: u16, is_out: bool, data: &[u8]) {
    let _ = (vcpu, port, is_out, data);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial::acquire;
    use crate::kvm::vm::{VmConfig, create_vm, reset_vm_manager, init_vm_manager};

    fn test_config() -> VmConfig {
        VmConfig {
            memory_size: 1024 * 1024,
            num_cpus: 1,
            kernel_entry: 0x1000_0000,
            initrd_start: 0x2000_0000,
            initrd_size: 0x1000,
        }
    }

    #[test]
    fn decode_hlt() {
        let _guard = acquire();
        assert_eq!(decode_exit(0x0C, 0), ExitReason::Hlt);
    }

    #[test]
    fn decode_cpuid() {
        let _guard = acquire();
        assert_eq!(decode_exit(0x0A, 0), ExitReason::Cpuid);
    }

    #[test]
    fn decode_triple_fault() {
        let _guard = acquire();
        assert_eq!(decode_exit(0x02, 0), ExitReason::TripleFault);
    }

    #[test]
    fn decode_io_instruction() {
        let _guard = acquire();
        assert_eq!(decode_exit(0x1E, 0), ExitReason::IoInstruction);
    }

    #[test]
    fn decode_unknown() {
        let _guard = acquire();
        assert_eq!(decode_exit(0xFF, 0), ExitReason::Unknown(0xFF));
    }

    #[test]
    fn cpuid_leaf_0_vendor() {
        let _guard = acquire();
        let (eax, ebx, ecx, edx) = handle_cpuid(0);
        assert_eq!(eax, 0x0000_000D);
        assert_eq!(ebx, 0x756E_6547);
        assert_eq!(ecx, 0x6C65_746E);
        assert_eq!(edx, 0x4965_6E69);
    }

    #[test]
    fn cpuid_leaf_1_features() {
        let _guard = acquire();
        let (eax, _, _, _) = handle_cpuid(1);
        assert_eq!(eax, 0x0003_06C3);
    }

    #[test]
    fn cpuid_unknown_leaf() {
        let _guard = acquire();
        let result = handle_cpuid(999);
        assert_eq!(result, (0, 0, 0, 0));
    }

    #[test]
    fn handle_hlt_sets_halted() {
        let _guard = acquire();
        reset_vm_manager();
        init_vm_manager();

        let config = test_config();
        let id = create_vm(&config).unwrap();
        let mut vm = super::super::vm::get_vm(id).unwrap();

        let exit = ExitInfo {
            reason: ExitReason::Hlt,
            qualification: 0,
            guest_rip: 0x1000,
            instruction_len: 1,
        };

        let cont = handle_exit(&mut vm, 0, exit);
        assert!(!cont);
        assert_eq!(vm.state, super::super::vm::VmState::Halted);
    }

    #[test]
    fn handle_triple_fault_sets_error() {
        let _guard = acquire();
        reset_vm_manager();
        init_vm_manager();

        let config = test_config();
        let id = create_vm(&config).unwrap();
        let mut vm = super::super::vm::get_vm(id).unwrap();

        let exit = ExitInfo {
            reason: ExitReason::TripleFault,
            qualification: 0,
            guest_rip: 0,
            instruction_len: 0,
        };

        let cont = handle_exit(&mut vm, 0, exit);
        assert!(!cont);
        assert_eq!(vm.state, super::super::vm::VmState::Error);
    }
}
