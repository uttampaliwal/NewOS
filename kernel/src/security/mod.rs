//! Security features for Turnix OS
//! Implements basic userspace isolation and capabilities

use spin::Mutex;

/// Capability rights
pub struct Capabilities {
    pub can_read: bool,
    pub can_write: bool,
    pub can_exec: bool,
    pub can_network: bool,
    pub can_admin: bool,
}

impl Capabilities {
    /// Full capabilities (for kernel/superuser)
    pub fn full() -> Self {
        Self {
            can_read: true,
            can_write: true,
            can_exec: true,
            can_network: true,
            can_admin: true,
        }
    }

    /// Basic user capabilities
    pub fn basic() -> Self {
        Self {
            can_read: true,
            can_write: true,
            can_exec: true,
            can_network: false,
            can_admin: false,
        }
    }

    /// Restricted capabilities
    pub fn restricted() -> Self {
        Self {
            can_read: true,
            can_write: false,
            can_exec: false,
            can_network: false,
            can_admin: false,
        }
    }
}

/// Process security context
pub struct SecurityContext {
    pub uid: u32,
    pub gid: u32,
    pub caps: Capabilities,
    pub is_privileged: bool,
}

static CURRENT_CONTEXT: Mutex<Option<SecurityContext>> = Mutex::new(None);

/// Initialize security subsystem
pub fn init() {
    crate::serial::println!("[SEC] Initializing security subsystem...");

    *CURRENT_CONTEXT.lock() = Some(SecurityContext {
        uid: 0, // Root user
        gid: 0,
        caps: Capabilities::full(),
        is_privileged: true,
    });

    crate::serial::println!("[SEC] Security subsystem initialized");
}

/// Set security context for a process
pub fn set_context(uid: u32, gid: u32, caps: Capabilities) {
    *CURRENT_CONTEXT.lock() = Some(SecurityContext {
        uid,
        gid,
        caps,
        is_privileged: uid == 0,
    });

    crate::serial::println!("[SEC] Context set: UID={}, GID={}", uid, gid);
}

/// Check if current process has a capability
pub fn check_capability(cap: &str) -> bool {
    let ctx = CURRENT_CONTEXT.lock();
    match &*ctx {
        Some(ctx) => match cap {
            "read" => ctx.caps.can_read,
            "write" => ctx.caps.can_write,
            "exec" => ctx.caps.can_exec,
            "network" => ctx.caps.can_network,
            "admin" => ctx.caps.can_admin,
            _ => false,
        },
        None => false,
    }
}

/// Get current user ID
pub fn get_uid() -> u32 {
    let ctx = CURRENT_CONTEXT.lock();
    match &*ctx {
        Some(ctx) => ctx.uid,
        None => 0, // Default to root
    }
}

/// Authenticate user (stub - would verify password)
pub fn authenticate(_username: &str, _password: &str) -> bool {
    // Stub implementation
    crate::serial::println!("[SEC] Authentication stub - always succeeds");
    true
}
