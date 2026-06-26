use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use spin::Mutex;

use crate::process::ProcessId;
use crate::security::namespaces::{
    MountNamespace, NetNamespace, PidNamespace, UserNamespace,
};
use crate::cgroup::{cgroup_create, cgroup_add_process};

use super::spec::{OciError, OciSpec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    Created,
    Running,
    Paused,
    Stopped,
    Deleted,
}

impl core::fmt::Display for ContainerState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ContainerState::Created => write!(f, "created"),
            ContainerState::Running => write!(f, "running"),
            ContainerState::Paused => write!(f, "paused"),
            ContainerState::Stopped => write!(f, "stopped"),
            ContainerState::Deleted => write!(f, "deleted"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Container {
    pub id: String,
    pub state: ContainerState,
    pub spec: OciSpec,
    pub pid_ns: PidNamespace,
    pub mnt_ns: MountNamespace,
    pub net_ns: NetNamespace,
    pub user_ns: UserNamespace,
    pub cgroup_path: String,
    pub init_pid: Option<ProcessId>,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerError {
    NotFound,
    InvalidState(String),
    AlreadyExists,
    ResourceError(String),
    SpecError(OciError),
}

pub struct ContainerManager {
    containers: BTreeMap<String, Container>,
    next_id: u64,
}

impl ContainerManager {
    fn new() -> Self {
        Self {
            containers: BTreeMap::new(),
            next_id: 1,
        }
    }

    fn generate_id(&mut self) -> String {
        let id = self.next_id;
        self.next_id += 1;
        let mut result = String::new();
        let mut val = id;
        let mut buf = [0u8; 20];
        let mut i = buf.len();
        if val == 0 {
            i -= 1;
            buf[i] = b'0';
        } else {
            while val > 0 {
                i -= 1;
                buf[i] = b'0' + (val % 10) as u8;
                val /= 10;
            }
        }
        let s = &buf[i..];
        for &b in s {
            result.push(b as char);
        }
        result
    }
}

static CONTAINER_MANAGER: Mutex<Option<ContainerManager>> = Mutex::new(None);

pub fn init_container_manager() {
    let mut mgr = CONTAINER_MANAGER.lock();
    *mgr = Some(ContainerManager::new());
}

pub fn reset_container_manager() {
    let mut mgr = CONTAINER_MANAGER.lock();
    *mgr = None;
}

pub fn create_container(spec: OciSpec) -> Result<String, ContainerError> {
    if let Err(e) = super::spec::validate(&spec) {
        return Err(ContainerError::SpecError(e));
    }

    let mut guard = CONTAINER_MANAGER.lock();
    let mgr = guard
        .as_mut()
        .ok_or_else(|| ContainerError::ResourceError("container manager not initialized".into()))?;

    let id = mgr.generate_id();

    if mgr.containers.contains_key(&id) {
        return Err(ContainerError::AlreadyExists);
    }

    let pid_ns = PidNamespace::new(None);
    let mnt_ns = MountNamespace::new();
    let net_ns = NetNamespace::new();
    let user_ns = UserNamespace::new();

    let cgroup_name = alloc::format!("/container_{}", id);
    if cgroup_create("/", &alloc::format!("container_{}", id)).is_err() {
        return Err(ContainerError::ResourceError(
            "failed to create cgroup".into(),
        ));
    }

    let container = Container {
        id: id.clone(),
        state: ContainerState::Created,
        spec,
        pid_ns,
        mnt_ns,
        net_ns,
        user_ns,
        cgroup_path: cgroup_name,
        init_pid: None,
        created_at: 0,
    };

    mgr.containers.insert(id.clone(), container);

    Ok(id)
}

pub fn start_container(id: &str) -> Result<(), ContainerError> {
    let mut guard = CONTAINER_MANAGER.lock();
    let mgr = guard
        .as_mut()
        .ok_or_else(|| ContainerError::ResourceError("container manager not initialized".into()))?;

    let container = mgr
        .containers
        .get_mut(id)
        .ok_or(ContainerError::NotFound)?;

    match container.state {
        ContainerState::Created => {
            let pid = ProcessId::new();
            container.init_pid = Some(pid);
            container.state = ContainerState::Running;

            let pid_num = pid.0 as u32;
            let _ = cgroup_add_process(&container.cgroup_path, pid_num);

            Ok(())
        }
        _ => Err(ContainerError::InvalidState(alloc::format!(
            "cannot start container in {} state",
            container.state
        ))),
    }
}

pub fn stop_container(id: &str) -> Result<(), ContainerError> {
    let mut guard = CONTAINER_MANAGER.lock();
    let mgr = guard
        .as_mut()
        .ok_or_else(|| ContainerError::ResourceError("container manager not initialized".into()))?;

    let container = mgr
        .containers
        .get_mut(id)
        .ok_or(ContainerError::NotFound)?;

    match container.state {
        ContainerState::Running | ContainerState::Paused => {
            container.state = ContainerState::Stopped;
            Ok(())
        }
        _ => Err(ContainerError::InvalidState(alloc::format!(
            "cannot stop container in {} state",
            container.state
        ))),
    }
}

pub fn pause_container(id: &str) -> Result<(), ContainerError> {
    let mut guard = CONTAINER_MANAGER.lock();
    let mgr = guard
        .as_mut()
        .ok_or_else(|| ContainerError::ResourceError("container manager not initialized".into()))?;

    let container = mgr
        .containers
        .get_mut(id)
        .ok_or(ContainerError::NotFound)?;

    match container.state {
        ContainerState::Running => {
            container.state = ContainerState::Paused;
            Ok(())
        }
        _ => Err(ContainerError::InvalidState(alloc::format!(
            "cannot pause container in {} state",
            container.state
        ))),
    }
}

pub fn resume_container(id: &str) -> Result<(), ContainerError> {
    let mut guard = CONTAINER_MANAGER.lock();
    let mgr = guard
        .as_mut()
        .ok_or_else(|| ContainerError::ResourceError("container manager not initialized".into()))?;

    let container = mgr
        .containers
        .get_mut(id)
        .ok_or(ContainerError::NotFound)?;

    match container.state {
        ContainerState::Paused => {
            container.state = ContainerState::Running;
            Ok(())
        }
        _ => Err(ContainerError::InvalidState(alloc::format!(
            "cannot resume container in {} state",
            container.state
        ))),
    }
}

pub fn delete_container(id: &str) -> Result<(), ContainerError> {
    let mut guard = CONTAINER_MANAGER.lock();
    let mgr = guard
        .as_mut()
        .ok_or_else(|| ContainerError::ResourceError("container manager not initialized".into()))?;

    let container = mgr
        .containers
        .get(id)
        .ok_or(ContainerError::NotFound)?;

    match container.state {
        ContainerState::Created | ContainerState::Stopped => {
            mgr.containers.remove(id);
            Ok(())
        }
        _ => Err(ContainerError::InvalidState(alloc::format!(
            "cannot delete container in {} state",
            container.state
        ))),
    }
}

pub fn get_container(id: &str) -> Option<Container> {
    let mgr = CONTAINER_MANAGER.lock();
    mgr.as_ref()?.containers.get(id).cloned()
}

pub fn list_containers() -> Vec<(String, ContainerState, u32)> {
    let mgr = CONTAINER_MANAGER.lock();
    let mgr = match mgr.as_ref() {
        Some(m) => m,
        None => return Vec::new(),
    };

    let mut result = Vec::new();
    for (id, container) in &mgr.containers {
        let pid_count = if container.init_pid.is_some() { 1 } else { 0 };
        result.push((id.clone(), container.state, pid_count));
    }
    result
}

pub fn container_count() -> usize {
    let mgr = CONTAINER_MANAGER.lock();
    match mgr.as_ref() {
        Some(m) => m.containers.len(),
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() {
        crate::cgroup::reset_for_test();
        init_container_manager();
    }

    fn teardown() {
        reset_container_manager();
    }

    #[test]
    fn init_and_reset_manager() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert_eq!(container_count(), 0);
        teardown();
    }

    #[test]
    fn create_container_succeeds() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = super::super::spec::parse_default_spec("/rootfs", &["/bin/sh"]);
        let id = create_container(spec);
        assert!(id.is_ok());
        assert_eq!(container_count(), 1);
        teardown();
    }

    #[test]
    fn create_container_assigns_sequential_ids() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = super::super::spec::parse_default_spec("/rootfs", &["/bin/sh"]);
        let id1 = create_container(spec.clone()).unwrap();
        let id2 = create_container(spec).unwrap();
        assert_ne!(id1, id2);
        teardown();
    }

    #[test]
    fn get_container_after_create() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = super::super::spec::parse_default_spec("/rootfs", &["/bin/sh"]);
        let id = create_container(spec).unwrap();
        let c = get_container(&id).unwrap();
        assert_eq!(c.state, ContainerState::Created);
        teardown();
    }

    #[test]
    fn get_container_not_found() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert!(get_container("nonexistent").is_none());
        teardown();
    }

    #[test]
    fn start_container_transitions_to_running() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = super::super::spec::parse_default_spec("/rootfs", &["/bin/sh"]);
        let id = create_container(spec).unwrap();
        assert!(start_container(&id).is_ok());
        let c = get_container(&id).unwrap();
        assert_eq!(c.state, ContainerState::Running);
        teardown();
    }

    #[test]
    fn start_container_not_found() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert_eq!(start_container("nope"), Err(ContainerError::NotFound));
        teardown();
    }

    #[test]
    fn stop_container_transitions_to_stopped() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = super::super::spec::parse_default_spec("/rootfs", &["/bin/sh"]);
        let id = create_container(spec).unwrap();
        start_container(&id).unwrap();
        assert!(stop_container(&id).is_ok());
        let c = get_container(&id).unwrap();
        assert_eq!(c.state, ContainerState::Stopped);
        teardown();
    }

    #[test]
    fn stop_container_invalid_state() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = super::super::spec::parse_default_spec("/rootfs", &["/bin/sh"]);
        let id = create_container(spec).unwrap();
        assert!(stop_container(&id).is_err());
        teardown();
    }

    #[test]
    fn pause_container_transitions_to_paused() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = super::super::spec::parse_default_spec("/rootfs", &["/bin/sh"]);
        let id = create_container(spec).unwrap();
        start_container(&id).unwrap();
        assert!(pause_container(&id).is_ok());
        let c = get_container(&id).unwrap();
        assert_eq!(c.state, ContainerState::Paused);
        teardown();
    }

    #[test]
    fn resume_container_transitions_to_running() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = super::super::spec::parse_default_spec("/rootfs", &["/bin/sh"]);
        let id = create_container(spec).unwrap();
        start_container(&id).unwrap();
        pause_container(&id).unwrap();
        assert!(resume_container(&id).is_ok());
        let c = get_container(&id).unwrap();
        assert_eq!(c.state, ContainerState::Running);
        teardown();
    }

    #[test]
    fn delete_container_after_stop() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = super::super::spec::parse_default_spec("/rootfs", &["/bin/sh"]);
        let id = create_container(spec).unwrap();
        start_container(&id).unwrap();
        stop_container(&id).unwrap();
        assert!(delete_container(&id).is_ok());
        assert_eq!(container_count(), 0);
        teardown();
    }

    #[test]
    fn delete_container_invalid_state() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = super::super::spec::parse_default_spec("/rootfs", &["/bin/sh"]);
        let id = create_container(spec).unwrap();
        start_container(&id).unwrap();
        assert_eq!(
            delete_container(&id),
            Err(ContainerError::InvalidState(
                "cannot delete container in running state".into()
            ))
        );
        teardown();
    }

    #[test]
    fn list_containers_returns_all() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = super::super::spec::parse_default_spec("/rootfs", &["/bin/sh"]);
        create_container(spec.clone()).unwrap();
        create_container(spec).unwrap();
        let list = list_containers();
        assert_eq!(list.len(), 2);
        teardown();
    }

    #[test]
    fn full_lifecycle() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = super::super::spec::parse_default_spec("/rootfs", &["/bin/sh"]);
        let id = create_container(spec).unwrap();

        let c = get_container(&id).unwrap();
        assert_eq!(c.state, ContainerState::Created);

        start_container(&id).unwrap();
        assert_eq!(get_container(&id).unwrap().state, ContainerState::Running);

        pause_container(&id).unwrap();
        assert_eq!(get_container(&id).unwrap().state, ContainerState::Paused);

        resume_container(&id).unwrap();
        assert_eq!(get_container(&id).unwrap().state, ContainerState::Running);

        stop_container(&id).unwrap();
        assert_eq!(get_container(&id).unwrap().state, ContainerState::Stopped);

        delete_container(&id).unwrap();
        assert!(get_container(&id).is_none());
        teardown();
    }
}
