# POSIX Capabilities

Turnix implements POSIX.1e draft capabilities using 64-bit capability sets.
This replaces the traditional root/non-root privilege model with fine-grained
per-process authorization.

---

## Capability Sets

Each process carries five capability bitmasks in its `SecurityContext`:

| Set | Purpose |
|-----|---------|
| **effective** | Capabilities checked during permission operations |
| **permitted** | Capabilities the process may use (superset of effective) |
| **inheritable** | Capabilities preserved across `exec()` |
| **bounding** | Upper limit on inheritable capabilities |
| **ambient** | Capabilities available to unprivileged processes |

### Capability Bits

43 Linux-compatible capabilities are defined (bit positions 0-42):

```
Bit  Name             Purpose
 0   CHOWN            Change file ownership
 1   DAC_OVERRIDE     Bypass file read/write/execute permissions
 2   DAC_READ_SEARCH  Bypass file read + directory search
 3   FOWNER           Bypass owner permission checks
 4   FSETID           Set file sticky/setuid bits
 5   KILL             Send signals to any process
 6   SETGID           Set real/effective GID
 7   SETUID           Set real/effective UID
 8   SETPCAP          Modify capability sets
 9   LINUX_IMMUTABLE  Set/clear immutable file attribute
10   NET_BIND_SERVICE Bind to ports < 1024
11   NET_BROADCAST     Broadcast sockets
12   NET_ADMIN          Network configuration
13   NET_RAW            Raw sockets
14   IPC_LOCK          Lock shared memory
15   IPC_OWNER         Bypass IPC ownership checks
16   SYS_MODULE        Load/unload kernel modules
17   SYS_RAWIO         Direct I/O port access
18   SYS_CHROOT        Use chroot()
19   SYS_PTRACE        Trace arbitrary processes
20   SYS_PACCT         Process accounting
21   SYS_ADMIN          Catch-all administrative operations
22   SYS_BOOT           Reboot system
23   SYS_NICE           Modify process scheduling
24   SYS_RESOURCE        Override resource limits
25   SYS_TIME           Set system clock
26   SYS_TTY_CONFIG     Configure TTY devices
27   MKNOD              Create device special files
28   LEASE              Establish file leases
29   AUDIT_WRITE        Write to kernel audit log
30   AUDIT_CONTROL      Configure audit subsystem
31   SETFCAP           Set file capabilities
32   MAC_OVERRIDE       Override MAC (Linux Security Module)
33   MAC_ADMIN          Administer MAC
34   SYSLOG             Read/write kernel log
35   WAKE_ALARM         Trigger wake-up alarm
36   BLOCK_SUSPEND       Block system suspend
37   AUDIT_READ          Read audit log
38   PERFMON             Performance monitoring
39   BPF                BPF operations
40   CHECKPOINT_RESTORE  Checkpoint/restore
41   PERFMON2            Extended performance monitoring
42   RESERVED            Reserved for future use
```

---

## Preset Configurations

```rust
CapabilitySet::root()      // All 43 bits set (full privileges)
CapabilitySet::basic()     // Chown, Kill, Setuid, NetBindService, etc.
                           // No SysAdmin, SysModule, or NetRaw
CapabilitySet::restricted() // Only DacReadSearch
CapabilitySet::empty()     // No capabilities
```

---

## Exec Transformation

On `exec()`, capabilities are transformed per POSIX rules:

### Standard Exec (no file capabilities)

```
P'(effective)   = 0
P'(permitted)   = (P(inheritable) ∩ F(inheritable)) ∪
                   (F(permitted) ∩ P(bounding))
P'(inheritable) = P(inheritable)
P'(ambient)     = (file is privileged) ? 0 : P(ambient)
```

### File Capabilities Exec

File capabilities are stored in the `security.capability` extended attribute
as a 24-byte little-endian structure. When present, they override the
inheritable set:

```
P'(permitted) = (P(inheritable) ∩ F(inheritable)) |
                (F(permitted) ∩ P(bounding))
```

---

## Ambient Capabilities

Ambient capabilities allow unprivileged processes to perform specific
operations without running as root. They are subject to:

1. Must be in both `permitted` and `inheritable` sets
2. Must be in the `bounding` set
3. Cleared on `exec()` if the binary has file capabilities or is setuid

---

## SecurityContext

Each process holds a `SecurityContext`:

```rust
pub struct SecurityContext {
    pub uid: u32,
    pub gid: u32,
    pub capabilities: CapabilitySet,
}
```

The `has_capability(cap)` method checks the `effective` set. If the
capability is not present, security-sensitive operations are denied.

---

## File Capabilities

File capabilities are stored as extended attributes and applied during exec:

```rust
pub struct FileCaps {
    pub permitted: u64,
    pub inheritable: u64,
}
```

Stored on disk as 24 bytes: 4 bytes magic + 8 bytes permitted + 8 bytes
inheritable + 4 bytes checksum.
