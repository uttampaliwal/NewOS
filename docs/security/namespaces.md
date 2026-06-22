# Namespaces

Turnix implements four Linux-compatible namespace types for process isolation.
Each namespace provides a separate view of a global resource.

---

## Namespace Types

### PID Namespace

**File:** `kernel/src/security/namespaces.rs` (lines 71-138)

Isolates the process ID number space. Processes in different PID namespaces
can have the same local PID.

- Each namespace has its own monotonic PID counter starting at 1
- PID 1 is the namespace init (reaps orphaned children)
- Local-to-global PID mapping via `BTreeMap<usize, ProcessId>`
- Parent namespace link enables cross-namespace signals

```
PID Namespace Hierarchy:
  Root NS (PID 1 = init)
    ├── Child NS (PID 1 = init, mapped to root PID 5)
    │     └── Grandchild NS (PID 1 = init, mapped to child PID 3)
    └── Child NS 2 (PID 1 = init, mapped to root PID 8)
```

### Mount Namespace

**File:** `kernel/src/security/namespaces.rs` (lines 139-198)

Isolates the filesystem mount table. Each process sees its own set of mounts.

- Forked via copy-on-write clone
- Mount/unmount operations only affect the current namespace
- Supports `pivot_root` for namespace init processes
- Mount propagation between namespaces (shared/private)

### Network Namespace

**File:** `kernel/src/security/namespaces.rs` (lines 201-228)

Isolates network interfaces and routing tables.

- Each namespace gets a unique `iface_index`
- Separate socket bindings per namespace
- Network configuration (DHCP, static IP) is namespace-scoped
- Loopback interface is per-namespace

### User Namespace

**File:** `kernel/src/security/namespaces.rs` (lines 229-293)

Maps UIDs and GIDs between the namespace and the parent namespace.

- Offset-based mapping: `(inside_uid, outside_uid, count)`
- Namespace root (UID 0 inside) maps to unprivileged UID outside
- Enables unprivileged operations within the namespace
- Supports nested user namespaces

---

## NsProxy

Each process holds an `NsProxy` bundling all four namespace types:

```rust
pub struct NsProxy {
    pub pid_ns: Option<PidNamespace>,
    pub mount_ns: Option<MountNamespace>,
    pub net_ns: Option<NetNamespace>,
    pub user_ns: Option<UserNamespace>,
}
```

When `None`, the process shares the parent's namespace. The effective
namespace is resolved by `effective_*_ns()` methods, which fall back
to root singletons.

---

## Creation via clone()

New namespaces are created using Linux-compatible flags:

| Flag | Value | Creates |
|------|-------|---------|
| `CLONE_NEWPID` | `0x20000000` | New PID namespace |
| `CLONE_NEWNS` | `0x00020000` | New mount namespace |
| `CLONE_NEWNET` | `0x40000000` | New network namespace |
| `CLONE_NEWUSER` | `0x10000000` | New user namespace |

Example: `clone(CLONE_NEWPID | CLONE_NEWNS)` creates both a PID and mount
namespace for the child process.

---

## Isolation Properties

| Resource | Without Namespace | With Namespace |
|----------|------------------|----------------|
| PIDs | Global, visible to all | Local to namespace |
| Mounts | Shared mount table | Per-namespace mount tree |
| Network | Single network stack | Per-namespace interfaces |
| UIDs | Global UID space | Mapped per-namespace |
