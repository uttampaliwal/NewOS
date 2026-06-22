# Linux Security Modules (LSM)

Turnix implements a pluggable LSM framework that intercepts security-sensitive
operations and enforces access control policies.

---

## Architecture

```
User Syscall
    │
    ▼
┌─────────────────────────────┐
│        LSM Hook Point       │
│   (file_open, process_      │
│    create, ipc_send,        │
│    net_connect, cap_check)  │
└─────────────────────────────┘
    │
    ▼
┌─────────────────────────────┐
│      LsmStack (ordered)     │
│   ┌─────────┐ ┌─────────┐  │
│   │ DacHook │ │MacHook  │  │
│   │ (DAC)   │ │ (MAC)   │  │
│   └─────────┘ └─────────┘  │
│   First Err wins (short-    │
│   circuit evaluation)       │
└─────────────────────────────┘
    │
    ▼
Operation Allowed / Denied
```

---

## Hook Points

| Hook | Called When | Parameters |
|------|-----------|------------|
| `file_open` | Opening a file | inode, flags, security context |
| `process_create` | fork/exec | parent context, child context |
| `ipc_send` | Sending IPC message | sender context, fd type |
| `net_connect` | Network connection | context, destination port |
| `capability_check` | Capability request | context, capability bit |

---

## Default Hooks

### DacHook (Discretionary Access Control)

The default DAC hook enforces Unix-style permissions:

| Hook | Rule |
|------|------|
| `file_open` | Deny write access to `/proc/` and `/sys/` for non-root |
| `ipc_send` | Deny shared-memory IPC (fd_type >= 2) for non-root |
| `net_connect` | Deny connections to ports < 1024 for non-root |
| `capability_check` | Verify capability is in effective set of SecurityContext |

### MacHook (Mandatory Access Control)

The MAC hook uses SELinux-style security labels:

```rust
pub struct SecurityLabel {
    pub user: String,    // e.g., "system_u"
    pub role: String,    // e.g., "system_r"
    pub level: String,   // e.g., "s0" (sensitivity level)
}
```

The `dominates()` check verifies that the subject's label dominates the
object's label using a simplified priority:
- `admin` > `user` > `default`

---

## LsmStack Evaluation

Hooks are evaluated in registration order. The first `Err(AccessDenied)`
from any hook denies the operation:

```rust
pub fn check(&self, hook_fn: impl Fn(&dyn LsmHook) -> Result<(), LsmError>)
    -> Result<(), LsmError>
{
    for hook in &self.hooks {
        hook_fn(hook.as_ref())?;
    }
    Ok(())
}
```

This short-circuit design means:
- Faster denial (stop at first failure)
- Hook ordering matters (most restrictive first)
- Adding new hooks never weakens existing policies

---

## Extension Points

New LSM modules implement the `LsmHook` trait:

```rust
pub trait LsmHook {
    fn file_open(&self, inode: u64, flags: u32, ctx: &SecurityContext)
        -> Result<(), LsmError>;
    fn process_create(&self, parent: &SecurityContext, child: &SecurityContext)
        -> Result<(), LsmError>;
    fn ipc_send(&self, sender: &SecurityContext, fd_type: u32)
        -> Result<(), LsmError>;
    fn net_connect(&self, ctx: &SecurityContext, port: u16)
        -> Result<(), LsmError>;
    fn capability_check(&self, ctx: &SecurityContext, cap: Capability)
        -> Result<(), LsmError>;
}
```

For MAC enforcement, implement the extended `MacHook` trait:

```rust
pub trait MacHook: LsmHook {
    fn mac_file_access(&self, subject: &SecurityLabel, object: &SecurityLabel)
        -> Result<(), LsmError>;
    fn mac_process_access(&self, subject: &SecurityLabel, target: &SecurityLabel)
        -> Result<(), LsmError>;
    fn mac_transition(&self, old: &SecurityLabel, new: &SecurityLabel)
        -> Result<(), LsmError>;
}
```

---

## Initialization

Both hooks are registered at boot:

```rust
pub fn init() {
    let mut stack = LSM_STACK.lock();
    stack.register(Box::new(DacHook));
    stack.register(Box::new(MacHookImpl));
}
```
