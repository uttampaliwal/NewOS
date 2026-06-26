use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use super::ept::Ept;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    Created,
    Running,
    Paused,
    Halted,
    Error,
}

#[derive(Debug, Clone)]
pub struct VmConfig {
    pub memory_size: usize,
    pub num_cpus: u32,
    pub kernel_entry: u64,
    pub initrd_start: u64,
    pub initrd_size: u64,
}

#[derive(Debug, Clone)]
pub struct Vcpu {
    pub id: u32,
    pub rip: u64,
    pub rsp: u64,
    pub cr0: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub rflags: u64,
    pub regs: [u64; 16],
}

impl Vcpu {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            rip: 0,
            rsp: 0,
            cr0: 0,
            cr3: 0,
            cr4: 0,
            rflags: 0,
            regs: [0u64; 16],
        }
    }
}

pub struct VirtualMachine {
    pub id: u32,
    pub config: VmConfig,
    pub state: VmState,
    pub ept: Option<Ept>,
    pub memory: Vec<u8>,
    pub vcpus: Vec<Vcpu>,
}

impl core::fmt::Debug for VirtualMachine {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VirtualMachine")
            .field("id", &self.id)
            .field("config", &self.config)
            .field("state", &self.state)
            .field("ept", &self.ept.is_some())
            .field("memory_len", &self.memory.len())
            .field("vcpus", &self.vcpus)
            .finish()
    }
}

impl Clone for VirtualMachine {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            config: self.config.clone(),
            state: self.state,
            ept: None,
            memory: self.memory.clone(),
            vcpus: self.vcpus.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvmError {
    OutOfMemory,
    InvalidConfig,
    VmNotFound,
    VcpuError,
    EptError,
}

pub struct VmManager {
    pub vms: Vec<VirtualMachine>,
    pub next_id: u32,
}

impl VmManager {
    pub fn new() -> Self {
        Self {
            vms: Vec::new(),
            next_id: 1,
        }
    }
}

static VM_MANAGER: Mutex<Option<VmManager>> = Mutex::new(None);

pub fn init_vm_manager() {
    let mut guard = VM_MANAGER.lock();
    if guard.is_none() {
        *guard = Some(VmManager::new());
    }
}

pub fn reset_vm_manager() {
    let mut guard = VM_MANAGER.lock();
    *guard = None;
}

pub fn create_vm(config: &VmConfig) -> Result<u32, KvmError> {
    if config.memory_size == 0 || config.num_cpus == 0 {
        return Err(KvmError::InvalidConfig);
    }

    let mut guard = VM_MANAGER.lock();
    let manager = guard.as_mut().ok_or(KvmError::VmNotFound)?;

    let id = manager.next_id;
    manager.next_id = manager.next_id.wrapping_add(1);

    let memory = vec![0u8; config.memory_size];
    let mut vcpus = Vec::new();
    for i in 0..config.num_cpus {
        vcpus.push(Vcpu::new(i));
    }

    let vm = VirtualMachine {
        id,
        config: config.clone(),
        state: VmState::Created,
        ept: None,
        memory,
        vcpus,
    };

    manager.vms.push(vm);
    Ok(id)
}

pub fn destroy_vm(id: u32) -> bool {
    let mut guard = VM_MANAGER.lock();
    let manager = match guard.as_mut() {
        Some(m) => m,
        None => return false,
    };

    let len = manager.vms.len();
    for i in 0..len {
        if manager.vms[i].id == id {
            manager.vms.swap_remove(i);
            return true;
        }
    }
    false
}

pub fn get_vm(id: u32) -> Option<VirtualMachine> {
    let guard = VM_MANAGER.lock();
    let manager = guard.as_ref()?;
    manager.vms.iter().find(|vm| vm.id == id).cloned()
}

pub fn start_vm(id: u32) -> bool {
    let mut guard = VM_MANAGER.lock();
    let manager = match guard.as_mut() {
        Some(m) => m,
        None => return false,
    };

    for vm in &mut manager.vms {
        if vm.id == id && vm.state == VmState::Created {
            vm.state = VmState::Running;
            return true;
        }
    }
    false
}

pub fn pause_vm(id: u32) -> bool {
    let mut guard = VM_MANAGER.lock();
    let manager = match guard.as_mut() {
        Some(m) => m,
        None => return false,
    };

    for vm in &mut manager.vms {
        if vm.id == id && vm.state == VmState::Running {
            vm.state = VmState::Paused;
            return true;
        }
    }
    false
}

pub fn resume_vm(id: u32) -> bool {
    let mut guard = VM_MANAGER.lock();
    let manager = match guard.as_mut() {
        Some(m) => m,
        None => return false,
    };

    for vm in &mut manager.vms {
        if vm.id == id && vm.state == VmState::Paused {
            vm.state = VmState::Running;
            return true;
        }
    }
    false
}

pub fn vm_count() -> usize {
    let guard = VM_MANAGER.lock();
    match guard.as_ref() {
        Some(m) => m.vms.len(),
        None => 0,
    }
}

pub fn list_vms() -> Vec<(u32, VmState, usize)> {
    let guard = VM_MANAGER.lock();
    match guard.as_ref() {
        Some(m) => m
            .vms
            .iter()
            .map(|vm| (vm.id, vm.state, vm.config.memory_size))
            .collect(),
        None => Vec::new(),
    }
}

pub fn vm_memory_mut(id: u32) -> Option<*mut [u8]> {
    let mut guard = VM_MANAGER.lock();
    let manager = guard.as_mut()?;
    let vm = manager.vms.iter_mut().find(|vm| vm.id == id)?;
    Some(vm.memory.as_mut_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial::acquire;

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
    fn vm_create_and_get() {
        let _guard = acquire();
        reset_vm_manager();
        init_vm_manager();

        let config = test_config();
        let id = create_vm(&config).unwrap();
        let vm = get_vm(id).unwrap();
        assert_eq!(vm.id, id);
        assert_eq!(vm.state, VmState::Created);
        assert_eq!(vm.config.memory_size, 1024 * 1024);
    }

    #[test]
    fn vm_state_transitions() {
        let _guard = acquire();
        reset_vm_manager();
        init_vm_manager();

        let config = test_config();
        let id = create_vm(&config).unwrap();

        assert!(start_vm(id));
        let vm = get_vm(id).unwrap();
        assert_eq!(vm.state, VmState::Running);

        assert!(pause_vm(id));
        let vm = get_vm(id).unwrap();
        assert_eq!(vm.state, VmState::Paused);

        assert!(resume_vm(id));
        let vm = get_vm(id).unwrap();
        assert_eq!(vm.state, VmState::Running);
    }

    #[test]
    fn vm_invalid_config_rejected() {
        let _guard = acquire();
        reset_vm_manager();
        init_vm_manager();

        let mut config = test_config();
        config.memory_size = 0;
        assert_eq!(create_vm(&config), Err(KvmError::InvalidConfig));

        let mut config = test_config();
        config.num_cpus = 0;
        assert_eq!(create_vm(&config), Err(KvmError::InvalidConfig));
    }

    #[test]
    fn vm_destroy() {
        let _guard = acquire();
        reset_vm_manager();
        init_vm_manager();

        let config = test_config();
        let id = create_vm(&config).unwrap();
        assert!(destroy_vm(id));
        assert!(get_vm(id).is_none());
        assert!(!destroy_vm(id));
    }

    #[test]
    fn vm_memory_access() {
        let _guard = acquire();
        reset_vm_manager();
        init_vm_manager();

        let config = test_config();
        let id = create_vm(&config).unwrap();
        let mem_ptr = vm_memory_mut(id).unwrap();
        // SAFETY: test only
        let mem = unsafe { &mut *mem_ptr };
        mem[0] = 0xAB;
        mem[1] = 0xCD;
        assert_eq!(mem[0], 0xAB);
        assert_eq!(mem[1], 0xCD);
    }

    #[test]
    fn vm_multi_vm() {
        let _guard = acquire();
        reset_vm_manager();
        init_vm_manager();

        let config = test_config();
        let id1 = create_vm(&config).unwrap();
        let id2 = create_vm(&config).unwrap();
        assert_ne!(id1, id2);
        assert_eq!(vm_count(), 2);
    }

    #[test]
    fn vm_list() {
        let _guard = acquire();
        reset_vm_manager();
        init_vm_manager();

        let config = test_config();
        create_vm(&config).unwrap();
        create_vm(&config).unwrap();
        let list = list_vms();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].1, VmState::Created);
        assert_eq!(list[1].1, VmState::Created);
    }

    #[test]
    fn vm_start_paused_fails() {
        let _guard = acquire();
        reset_vm_manager();
        init_vm_manager();

        let config = test_config();
        let id = create_vm(&config).unwrap();
        assert!(!pause_vm(id));
        assert!(!resume_vm(id));
    }

    #[test]
    fn vm_not_found() {
        let _guard = acquire();
        reset_vm_manager();
        init_vm_manager();

        assert!(get_vm(999).is_none());
        assert!(!start_vm(999));
        assert!(!pause_vm(999));
        assert!(!resume_vm(999));
        assert!(!destroy_vm(999));
    }

    #[test]
    fn vm_vcpu_count_matches_config() {
        let _guard = acquire();
        reset_vm_manager();
        init_vm_manager();

        let mut config = test_config();
        config.num_cpus = 4;
        let id = create_vm(&config).unwrap();
        let vm = get_vm(id).unwrap();
        assert_eq!(vm.vcpus.len(), 4);
        for (i, vcpu) in vm.vcpus.iter().enumerate() {
            assert_eq!(vcpu.id, i as u32);
        }
    }

    #[test]
    fn vm_init_twice_is_idempotent() {
        let _guard = acquire();
        reset_vm_manager();
        init_vm_manager();
        init_vm_manager();
        assert_eq!(vm_count(), 0);
    }

    #[test]
    fn vm_reset_clears_all() {
        let _guard = acquire();
        reset_vm_manager();
        init_vm_manager();

        let config = test_config();
        create_vm(&config).unwrap();
        create_vm(&config).unwrap();
        assert_eq!(vm_count(), 2);

        reset_vm_manager();
        init_vm_manager();
        assert_eq!(vm_count(), 0);
    }
}
