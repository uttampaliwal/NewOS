//! Security features for Turnix OS
//! Implements POSIX 64-bit capabilities and basic security context.

pub mod capabilities;
pub mod namespaces;
pub mod seccomp;
pub mod lsm;
pub mod ima;

use capabilities::CapabilitySet;

use spin::Mutex;

/// Process security context (per-process, stored in ProcessControlBlock).
#[derive(Debug, Clone)]
pub struct SecurityContext {
    pub uid: u32,
    pub gid: u32,
    pub caps: CapabilitySet,
    pub is_privileged: bool,
}

impl SecurityContext {
    pub fn root() -> Self {
        Self {
            uid: 0,
            gid: 0,
            caps: CapabilitySet::root(),
            is_privileged: true,
        }
    }

    pub fn new(uid: u32, gid: u32, caps: CapabilitySet) -> Self {
        Self {
            uid,
            gid,
            caps,
            is_privileged: uid == 0,
        }
    }

    pub fn has_capability(&self, cap: capabilities::Capability) -> bool {
        self.caps.has(cap)
    }

    pub fn has_effective(&self, cap: capabilities::Capability) -> bool {
        self.caps.has(cap)
    }

    pub fn has_permitted(&self, cap: capabilities::Capability) -> bool {
        self.caps.has_permitted(cap)
    }
}

// Global fallback context for kernel threads / boot phase.
static CURRENT_CONTEXT: Mutex<Option<SecurityContext>> = Mutex::new(None);

/// Initialize security subsystem
pub fn init() {
    crate::serial::println!("[SEC] Initializing security subsystem...");

    *CURRENT_CONTEXT.lock() = Some(SecurityContext::root());
    crate::serial::println!("[SEC] capabilities initialized");

    crate::security::lsm::init();
    crate::security::ima::init();
    crate::security::seccomp::init();

    crate::serial::println!("[SEC] Security subsystem initialized");
}

/// Set the global fallback security context (used for kernel threads).
pub fn set_context(uid: u32, gid: u32, caps: CapabilitySet) {
    *CURRENT_CONTEXT.lock() = Some(SecurityContext::new(uid, gid, caps));

    crate::serial::println!("[SEC] Context set: UID={}, GID={}", uid, gid);
}

/// Get the global fallback context.
pub fn current_context() -> SecurityContext {
    CURRENT_CONTEXT
        .lock()
        .clone()
        .unwrap_or_else(SecurityContext::root)
}

/// Check if current context has a specific POSIX capability.
pub fn check_capability(cap: capabilities::Capability) -> bool {
    let ctx = CURRENT_CONTEXT.lock();
    match &*ctx {
        Some(ctx) => ctx.has_capability(cap),
        None => true, // Default to root
    }
}

/// Get current user ID from global context.
pub fn get_uid() -> u32 {
    let ctx = CURRENT_CONTEXT.lock();
    match &*ctx {
        Some(ctx) => ctx.uid,
        None => 0,
    }
}

/// Get current group ID from global context.
pub fn get_gid() -> u32 {
    let ctx = CURRENT_CONTEXT.lock();
    match &*ctx {
        Some(ctx) => ctx.gid,
        None => 0,
    }
}

/// Authenticate user (stub - would verify password)
pub fn authenticate(_username: &str, _password: &str) -> bool {
    crate::serial::println!("[SEC] Authentication stub - always succeeds");
    true
}
