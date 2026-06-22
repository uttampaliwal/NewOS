# Turnix Threat Model

This document defines the trust boundaries, attacker models, and Trusted
Computing Base (TCB) for the Turnix operating system.

---

## Trust Boundaries

```
+-----------------------------------------------------------+
|                    UNTRUSTED ZONE                         |
|   User applications, third-party packages, network input  |
+-----------------------------------------------------------+
          |  Syscall Interface (int 0x81 / syscall)
          |  Seccomp-BPF Filter
          |  Capability Checks
          |  LSM Hooks
+-----------------------------------------------------------+
|                    KERNEL SPACE (TCB)                     |
|   Scheduler | VFS | VMM | IPC | Security | Drivers       |
+-----------------------------------------------------------+
          |  Hardware Abstraction
+-----------------------------------------------------------+
|                    HARDWARE / FIRMWARE                    |
|   UEFI | TPM 2.0 | NVMe | PCIe | x86_64 CPU             |
+-----------------------------------------------------------+
```

### Boundary 1: User -> Kernel (Syscall Boundary)

Every system call crosses from Ring 3 to Ring 0. This is the primary attack
surface. Enforcement layers (applied in order):

1. **Seccomp-BPF filter** — syscall number and arguments checked against
   user-supplied BPF program. Deny-by-default for filtered processes.
2. **Capability check** — process must hold the required POSIX capability in
   its effective set.
3. **LSM hooks** — DAC and MAC policies evaluated. Short-circuit on first
   denial.
4. **Argument validation** — kernel validates all pointers, lengths, and
   flags before dereferencing.

### Boundary 2: Kernel -> Driver

Drivers run in kernel space but are isolated via trait boundaries
(`DeviceDriver` trait with `probe`, `initialize`, `suspend`, `resume`).
A driver bug can corrupt kernel memory, but the trait interface limits
the blast radius.

### Boundary 3: Process -> Process (IPC)

Pipes and Unix domain sockets mediate inter-process communication.
Each IPC channel is bound to a file descriptor with standard permission
checks. Namespace isolation prevents cross-namespace IPC unless explicitly
shared.

### Boundary 4: Kernel -> Hardware

The UEFI loader establishes initial page tables and hands off via `BootInfo`.
KASLR randomizes the kernel load address. The kernel trusts no hardware
input without validation (ACPI tables are checksummed, PCIe devices are
enumerated defensively).

---

## Attacker Models

### Model 1: Malicious Userspace Application

**Capability:** Ring 3 code execution, can make arbitrary syscalls.
**Goal:** Escalate privileges, read kernel memory, corrupt other processes.

**Defenses:**
- Seccomp-BPF restricts available syscalls
- POSIX capabilities limit privileged operations
- Namespaces isolate PID/mount/network/user views
- ASLR randomizes load/stack/heap bases (2^28 positions each)
- Stack canaries detect kernel stack buffer overflows
- W^X prevents code injection via writable+executable pages

### Model 2: Compromised Service Process

**Capability:** Ring 3 code execution with service-level privileges.
**Goal:** Access other services' data, tamper with filesystem.

**Defenses:**
- Mount namespaces isolate filesystem views
- LSM DAC hook denies non-root writes to /proc/ and /sys/
- LSM MAC hook enforces label-based access control
- File capabilities restrict inherited capabilities on exec
- IMA measures every executed binary (audit trail)

### Model 3: Network-Based Attack

**Capability:** Send arbitrary network packets to VirtIO-Net interface.
**Goal:** Remote code execution, denial of service.

**Defenses:**
- Seccomp-BPF filters on network-facing services
- LSM hook denies non-root connections to privileged ports (<1024)
- Network namespace isolation
- smoltcp TCP/IP stack validates all packet headers

### Model 4: Persistent Compromise (Evil Maid)

**Capability:** Physical access to disk/TPM.
**Goal:** Tamper with kernel or filesystem images.

**Defenses:**
- IMA measures all executed binaries (detects tampered images)
- EVM verifies file metadata integrity via HMAC-SHA256
- TPM 2.0 seals/unseals cryptographic keys
- KASLR randomizes kernel load address per boot

---

## Trusted Computing Base (TCB)

The TCB includes everything that must be correct for security properties to
hold. In Turnix, the TCB is:

| Component | Why in TCB | Lines of Unsafe Rust |
|-----------|------------|---------------------|
| Scheduler | Context switches, CR3 swaps | ~200 |
| VMM | Page table manipulation | ~400 |
| Syscall handler | Ring 3 -> Ring 0 transition | ~1500 |
| Security framework | Capability/LSM/seccomp checks | ~800 |
| IPC (pipes, sockets) | Cross-process data flow | ~600 |
| ELF loader | Binary loading, ASLR | ~300 |
| UEFI boot handoff | Initial page tables, KASLR | ~500 |

**Out of TCB:** Userland processes, device drivers (trait-bounded), the
compositor, and all daemons. A bug in a userland process cannot compromise
kernel integrity (by design).

---

## Security Properties

| Property | Mechanism | Guarantee |
|----------|-----------|-----------|
| Process isolation | Namespaces + capabilities | One process cannot affect another's resources without authorization |
| Memory safety | Rust ownership + W^X + ASLR | No arbitrary code execution via buffer overflows |
| Privilege escalation prevention | Capability inheritance rules | Ambitious caps cannot exceed bounding set |
| Syscall restriction | Seccomp-BPF | Processes can only use explicitly allowed syscalls |
| Integrity measurement | IMA/EVM + TPM | All loaded code is hash-recorded; file metadata is HMAC-verified |
| Stack protection | Canary verification on context switch | Stack buffer overflows are detected and cause immediate panic |
| Audit trail | IMA measurement log | 4096-entry ring buffer of all executed binary hashes |
