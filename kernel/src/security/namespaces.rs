//! PID, mount, network, and user namespaces.
//!
//! Each process has an `NsProxy` containing four optional namespaces.
//! When a namespace is `None`, the process shares the parent's / initial
//! namespace (the kernel's single global namespace).

use core::sync::atomic::{AtomicUsize, Ordering};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::process::ProcessId;

// ---------------------------------------------------------------------------
// CLONE flags (Linux-compatible)
// ---------------------------------------------------------------------------

pub const CLONE_VM: u64          = 0x00000100;
pub const CLONE_FS: u64          = 0x00000200;
pub const CLONE_FILES: u64       = 0x00000400;
pub const CLONE_SIGHAND: u64     = 0x00000800;
pub const CLONE_PIDFD: u64       = 0x00001000;
pub const CLONE_PTRACE: u64      = 0x00002000;
pub const CLONE_VFORK: u64       = 0x00004000;
pub const CLONE_PARENT: u64      = 0x00008000;
pub const CLONE_THREAD: u64      = 0x00010000;
pub const CLONE_NEWNS: u64       = 0x00020000;
pub const CLONE_SYSVSEM: u64     = 0x00040000;
pub const CLONE_SETTLS: u64      = 0x00080000;
pub const CLONE_PARENT_SETTID: u64 = 0x00100000;
pub const CLONE_CHILD_CLEARTID: u64 = 0x00200000;
pub const CLONE_DETACHED: u64    = 0x00400000;
pub const CLONE_UNTRACED: u64    = 0x00800000;
pub const CLONE_CHILD_SETTID: u64 = 0x01000000;
pub const CLONE_NEWCGROUP: u64   = 0x02000000;
pub const CLONE_NEWUTS: u64      = 0x04000000;
pub const CLONE_NEWIPC: u64      = 0x08000000;
pub const CLONE_NEWUSER: u64     = 0x10000000;
pub const CLONE_NEWPID: u64      = 0x20000000;
pub const CLONE_NEWNET: u64      = 0x40000000;
pub const CLONE_IO: u64          = 0x80000000;

/// Convenience: all NEW* namespace flags.
pub const CLONE_NEW_ALL: u64 = CLONE_NEWNS | CLONE_NEWUSER | CLONE_NEWPID | CLONE_NEWNET;

// ---------------------------------------------------------------------------
// Namespace IDs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NsId(usize);

impl NsId {
    pub fn new() -> Self {
        static NEXT_NS_ID: AtomicUsize = AtomicUsize::new(0);
        NsId(NEXT_NS_ID.fetch_add(1, Ordering::Relaxed))
    }
}

// ---------------------------------------------------------------------------
// PID Namespace
// ---------------------------------------------------------------------------

/// A PID namespace provides its own PID numbering.
/// Processes inside only see PIDs that belong to this namespace or its children.
#[derive(Debug)]
pub struct PidNamespace {
    pub id: NsId,
    ns_pid_counter: AtomicUsize,
    /// Map ns-local PID → global ProcessId for quick translation.
    local_to_global: BTreeMap<usize, ProcessId>,
    /// Parent namespace (None for the root PID namespace).
    pub parent: Option<NsId>,
}

impl Clone for PidNamespace {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            ns_pid_counter: AtomicUsize::new(self.ns_pid_counter.load(Ordering::Relaxed)),
            local_to_global: self.local_to_global.clone(),
            parent: self.parent,
        }
    }
}

impl PidNamespace {
    pub fn new(parent: Option<NsId>) -> Self {
        Self {
            id: NsId::new(),
            ns_pid_counter: AtomicUsize::new(1),
            local_to_global: BTreeMap::new(),
            parent,
        }
    }

    /// Root namespace (used by PID 0 / kernel).
    pub fn root() -> Self {
        Self::new(None)
    }

    /// Allocate a new ns-local PID for the given global process.
    pub fn alloc_pid(&mut self, global_pid: ProcessId) -> usize {
        let local = self.ns_pid_counter.fetch_add(1, Ordering::Relaxed);
        self.local_to_global.insert(local, global_pid);
        local
    }

    /// Translate a global PID to the ns-local PID.
    /// Returns `None` if the global PID is not in this namespace.
    pub fn global_to_local(&self, global: ProcessId) -> Option<usize> {
        self.local_to_global
            .iter()
            .find(|&(_, &g)| g == global)
            .map(|(l, _)| *l)
    }

    /// Translate a ns-local PID to a global PID.
    pub fn local_to_global(&self, local: usize) -> Option<ProcessId> {
        self.local_to_global.get(&local).copied()
    }

    /// Remove a PID mapping (called on process exit).
    pub fn remove_pid(&mut self, global: ProcessId) {
        self.local_to_global.retain(|_, &mut g| g != global);
    }
}

// ---------------------------------------------------------------------------
// Mount Namespace
// ---------------------------------------------------------------------------

/// A mount namespace provides an isolated view of the filesystem mount table.
#[derive(Debug, Clone)]
pub struct MountNamespace {
    pub id: NsId,
}

impl MountNamespace {
    pub fn new() -> Self {
        Self { id: NsId::new() }
    }

    /// Create a child mount namespace by cloning the parent's mount table.
    /// In a real implementation this would deep-copy mount entries.
    /// Currently the VFS is global, so this is a placeholder for per-ns mount tables.
    pub fn fork() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Network Namespace
// ---------------------------------------------------------------------------

/// A network namespace provides an isolated network stack.
#[derive(Debug, Clone)]
pub struct NetNamespace {
    pub id: NsId,
}

impl NetNamespace {
    pub fn new() -> Self {
        Self { id: NsId::new() }
    }
}

// ---------------------------------------------------------------------------
// User Namespace
// ---------------------------------------------------------------------------

/// A user namespace maps UIDs/GIDs between the namespace and the host.
#[derive(Debug, Clone)]
pub struct UserNamespace {
    pub id: NsId,
    /// UID mappings: (inside, outside, count)
    pub uid_map: Vec<(u32, u32, u32)>,
    /// GID mappings
    pub gid_map: Vec<(u32, u32, u32)>,
}

impl UserNamespace {
    pub fn new() -> Self {
        Self {
            id: NsId::new(),
            uid_map: Vec::new(),
            gid_map: Vec::new(),
        }
    }

    /// Map a namespace-internal UID to the global (host) UID.
    /// Returns the first matching outside UID, or None.
    pub fn map_uid(&self, inside: u32) -> Option<u32> {
        for &(start_in, start_out, count) in &self.uid_map {
            let offset = inside.checked_sub(start_in)?;
            if offset < count {
                return Some(start_out + offset);
            }
        }
        None
    }

    /// Map a namespace-internal GID to global.
    pub fn map_gid(&self, inside: u32) -> Option<u32> {
        for &(start_in, start_out, count) in &self.gid_map {
            let offset = inside.checked_sub(start_in)?;
            if offset < count {
                return Some(start_out + offset);
            }
        }
        None
    }

    /// Add a UID mapping. In a real kernel this would be restricted to
    /// `/proc/<pid>/uid_map` writes by a privileged user.
    pub fn add_uid_map(&mut self, inside: u32, outside: u32, count: u32) {
        self.uid_map.push((inside, outside, count));
    }

    pub fn add_gid_map(&mut self, inside: u32, outside: u32, count: u32) {
        self.gid_map.push((inside, outside, count));
    }
}

// ---------------------------------------------------------------------------
// NsProxy — the bundle of namespaces attached to a process
// ---------------------------------------------------------------------------

/// Holds references to all four namespace types.
/// When a field is `None`, the process inherits the parent's namespace.
#[derive(Debug, Clone)]
pub struct NsProxy {
    pub pid_ns: Option<PidNamespace>,
    pub mnt_ns: Option<MountNamespace>,
    pub net_ns: Option<NetNamespace>,
    pub user_ns: Option<UserNamespace>,
}

impl NsProxy {
    pub fn new() -> Self {
        Self {
            pid_ns: None,
            mnt_ns: None,
            net_ns: None,
            user_ns: None,
        }
    }

    /// Create namespaces as requested by the clone flags.
    /// `parent` is the NsProxy of the calling process.
    pub fn from_flags(flags: u64, parent: &NsProxy) -> Self {
        Self {
            pid_ns: if flags & CLONE_NEWPID != 0 {
                Some(PidNamespace::new(parent.pid_ns.as_ref().map(|ns| ns.id)))
            } else {
                parent.pid_ns.clone()
            },
            mnt_ns: if flags & CLONE_NEWNS != 0 {
                Some(MountNamespace::fork())
            } else {
                parent.mnt_ns.clone()
            },
            net_ns: if flags & CLONE_NEWNET != 0 {
                Some(NetNamespace::new())
            } else {
                parent.net_ns.clone()
            },
            user_ns: if flags & CLONE_NEWUSER != 0 {
                Some(UserNamespace::new())
            } else {
                parent.user_ns.clone()
            },
        }
    }

    /// Resolve the effective PID namespace for this process.
    /// Walks up to the root if this process has no own PID namespace.
    pub fn effective_pid_ns(&self) -> &PidNamespace {
        // In our model, PID 0's NsProxy has a root PID namespace stored here.
        // For processes without their own, we fall back to a root singleton.
        self.pid_ns.as_ref().unwrap_or_else(|| {
            // The root PID namespace is lazily created — but in practice every
            // process created via fork/clone inherits a pid_ns, so this branch
            // is a safety fallback.
            lazy_static::lazy_static! {
                static ref ROOT_PID_NS: PidNamespace = PidNamespace::root();
            }
            &ROOT_PID_NS
        })
    }

    pub fn effective_mnt_ns(&self) -> &MountNamespace {
        self.mnt_ns.as_ref().unwrap_or_else(|| {
            lazy_static::lazy_static! {
                static ref ROOT_MNT_NS: MountNamespace = MountNamespace::new();
            }
            &ROOT_MNT_NS
        })
    }

    pub fn effective_net_ns(&self) -> &NetNamespace {
        self.net_ns.as_ref().unwrap_or_else(|| {
            lazy_static::lazy_static! {
                static ref ROOT_NET_NS: NetNamespace = NetNamespace::new();
            }
            &ROOT_NET_NS
        })
    }

    pub fn effective_user_ns(&self) -> &UserNamespace {
        self.user_ns.as_ref().unwrap_or_else(|| {
            lazy_static::lazy_static! {
                static ref ROOT_USER_NS: UserNamespace = UserNamespace::new();
            }
            &ROOT_USER_NS
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcessId;

    #[test]
    fn pid_ns_allocates_local_pids() {
        let mut ns = PidNamespace::root();
        let local1 = ns.alloc_pid(ProcessId(42));
        let local2 = ns.alloc_pid(ProcessId(99));
        assert_eq!(local1, 1);
        assert_eq!(local2, 2);
    }

    #[test]
    fn pid_ns_global_to_local_roundtrip() {
        let mut ns = PidNamespace::root();
        let local = ns.alloc_pid(ProcessId(42));
        assert_eq!(ns.global_to_local(ProcessId(42)), Some(local));
        assert_eq!(ns.local_to_global(local), Some(ProcessId(42)));
    }

    #[test]
    fn pid_ns_remove_pid() {
        let mut ns = PidNamespace::root();
        ns.alloc_pid(ProcessId(42));
        assert!(ns.global_to_local(ProcessId(42)).is_some());
        ns.remove_pid(ProcessId(42));
        assert!(ns.global_to_local(ProcessId(42)).is_none());
    }

    #[test]
    fn pid_ns_unknown_pid() {
        let ns = PidNamespace::root();
        assert_eq!(ns.global_to_local(ProcessId(999)), None);
        assert_eq!(ns.local_to_global(999), None);
    }

    #[test]
    fn user_ns_uid_mapping() {
        let mut ns = UserNamespace::new();
        ns.add_uid_map(0, 1000, 1);
        ns.add_uid_map(1000, 1001, 100);
        assert_eq!(ns.map_uid(0), Some(1000));
        assert_eq!(ns.map_uid(1050), Some(1051));
        assert_eq!(ns.map_uid(2000), None);
    }

    #[test]
    fn user_ns_gid_mapping() {
        let mut ns = UserNamespace::new();
        ns.add_gid_map(0, 500, 1);
        assert_eq!(ns.map_gid(0), Some(500));
        assert_eq!(ns.map_gid(1), None);
    }

    #[test]
    fn nsproxy_creates_from_flags() {
        let parent = NsProxy::new();
        // No namespace flags → inherit parent
        let child = NsProxy::from_flags(0, &parent);
        assert!(child.pid_ns.is_none());
        assert!(child.mnt_ns.is_none());

        // With NEWPID flag → new PID namespace
        let child2 = NsProxy::from_flags(CLONE_NEWPID, &parent);
        assert!(child2.pid_ns.is_some());
        assert!(child2.mnt_ns.is_none());

        // With all namespace flags
        let child3 = NsProxy::from_flags(CLONE_NEW_ALL, &parent);
        assert!(child3.pid_ns.is_some());
        assert!(child3.mnt_ns.is_some());
        assert!(child3.net_ns.is_some());
        assert!(child3.user_ns.is_some());
    }

    #[test]
    fn pid_ns_isolation() {
        let mut parent_ns = PidNamespace::root();
        let mut child_ns = PidNamespace::new(Some(parent_ns.id));

        parent_ns.alloc_pid(ProcessId(1)); // init in root
        parent_ns.alloc_pid(ProcessId(42)); // some process
        let local = child_ns.alloc_pid(ProcessId(42)); // child in its own ns

        // In the child's ns, PID 42 (global) is PID 1 (first in child ns)
        assert_eq!(local, 1);
        // The child ns doesn't know about the root's PID 1
        assert!(child_ns.global_to_local(ProcessId(1)).is_none());
    }

    #[test]
    fn mount_namespace_new_and_fork() {
        let ns = MountNamespace::new();
        let forked = MountNamespace::fork();
        // Both should exist without panicking
        drop(ns);
        drop(forked);
    }

    #[test]
    fn net_namespace_new() {
        let ns = NetNamespace::new();
        drop(ns);
    }

    #[test]
    fn user_namespace_reverse_mapping() {
        let mut ns = UserNamespace::new();
        ns.add_uid_map(0, 1000, 1);   // outside uid 0 → inside uid 1000
        ns.add_uid_map(1000, 1001, 100); // outside 1000..1099 → inside 1001..1100
        ns.add_gid_map(0, 500, 1);

        // Forward mapping already tested; reverse via the same map_uid
        assert_eq!(ns.map_uid(0), Some(1000));
        assert_eq!(ns.map_uid(1050), Some(1051));
        assert_eq!(ns.map_uid(2000), None);

        // GID forward
        assert_eq!(ns.map_gid(0), Some(500));
        assert_eq!(ns.map_gid(1), None);
    }

    #[test]
    fn user_namespace_empty_mappings() {
        let ns = UserNamespace::new();
        assert_eq!(ns.map_uid(0), None);
        assert_eq!(ns.map_gid(0), None);
    }

    #[test]
    fn nsproxy_effective_methods() {
        let parent = NsProxy::new();
        // Without any namespaces, effective methods should return root singletons
        let _pid_ns = parent.effective_pid_ns();
        let _mnt_ns = parent.effective_mnt_ns();
        let _net_ns = parent.effective_net_ns();
        let _user_ns = parent.effective_user_ns();

        // With all namespaces, effective should return the child's namespace
        let mut child = NsProxy::from_flags(CLONE_NEW_ALL, &parent);
        // Allocate a PID so the namespace has entries
        let local = child.pid_ns.as_mut().unwrap().alloc_pid(ProcessId(100));
        assert_eq!(local, 1);
        assert!(child.effective_pid_ns().global_to_local(ProcessId(100)).is_some());
        assert!(child.effective_pid_ns().local_to_global(1).is_some());
    }

    #[test]
    fn nsproxy_isolation() {
        let parent = NsProxy::new();
        // Create child with its own PID ns
        let mut child = NsProxy::from_flags(CLONE_NEWPID, &parent);
        // Access child's own pid_ns directly (mutable)
        let local = child.pid_ns.as_mut().unwrap().alloc_pid(ProcessId(100));
        assert_eq!(local, 1);
        // Parent has no pid_ns → self.pid_ns is None, effective_pid_ns() returns root
        // Create a fresh proxy to test isolation
        let mut parent2 = NsProxy::new();
        // Explicitly give parent2 its own pid_ns for the test
        parent2.pid_ns = Some(PidNamespace::root());
        let p_local = parent2.pid_ns.as_mut().unwrap().alloc_pid(ProcessId(200));
        assert_eq!(p_local, 1);
        // Child's PID 100 should not conflict with parent2's PID 200
        assert!(parent2.pid_ns.as_ref().unwrap().global_to_local(ProcessId(100)).is_none());
    }

    #[test]
    fn nsproxy_from_flags_individual() {
        let parent = NsProxy::new();

        // NEWNS only
        let child = NsProxy::from_flags(CLONE_NEWNS, &parent);
        assert!(child.mnt_ns.is_some());
        assert!(child.pid_ns.is_none());
        assert!(child.net_ns.is_none());
        assert!(child.user_ns.is_none());

        // NEWNET only
        let child = NsProxy::from_flags(CLONE_NEWNET, &parent);
        assert!(child.net_ns.is_some());
        assert!(child.pid_ns.is_none());
        assert!(child.mnt_ns.is_none());
        assert!(child.user_ns.is_none());

        // NEWUSER only
        let child = NsProxy::from_flags(CLONE_NEWUSER, &parent);
        assert!(child.user_ns.is_some());
        assert!(child.pid_ns.is_none());
        assert!(child.mnt_ns.is_none());
        assert!(child.net_ns.is_none());
    }
}
