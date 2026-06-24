# Kernel

The `kernel` crate is the core of Turnix OS — a no_std, higher-half, x86_64 monolithic kernel.

## Modules

| Module | Description |
|--------|-------------|
| `acpi` | ACPI table parsing (RSDP, XSDT, MCFG, DSDT/SSDT) and AML interpreter |
| `arch` | Architecture-specific code (GDT, IDT, TSS, context switching) |
| `block` | Block device I/O layer (`BlockDevice` trait, block cache) |
| `boot` | Early boot initialization, driver probing, stage tracking |
| `cgroup` | cgroups v2 hierarchy with CPU, memory, and PIDs controllers |
| `drivers` | Device drivers: PCI/PCIe, VirtIO-Net, NVMe, XHCI USB, GPU/DRM, TPM |
| `elf` | ELF binary loader for userspace processes |
| `fs` | Virtual File System (VFS), tmpfs, in-memory ext4 state, mount management |
| `ipc` | Pipes, Unix domain sockets, epoll, futex, POSIX message queues, shared memory |
| `log_ring` | Kernel log ring buffer with dmesg syscall |
| `memory` | VMM, page tables, heap allocator, page cache, swap, OOM killer, slab allocator |
| `net` | smoltcp-based TCP/IP stack, socket syscalls, network interface config |
| `process` | Process table, fork/exec/wait, signal handling, file descriptor tables |
| `security` | Capabilities, namespaces, seccomp-BPF, LSM hooks, IMA/EVM |
| `serial` | Serial port (COM1) output for kernel logging |
| `smp` | Symmetric multiprocessing (AP bring-up, per-CPU scheduling) |
| `syscall` | System call dispatch and handler (78 syscalls) |
| `task` | Task structures, CFS vruntime scheduler, scheduler classes |
| `time` | Monotonic clock (uptime in microseconds) |
| `tty` | Terminal I/O |

## Testing

```bash
cargo test -p turnix-kernel
```

## Known Limitations

See [KNOWN_ISSUES.md](../KNOWN_ISSUES.md) for all tracked issues. Key kernel-specific items:
- **#1** ext4 writes are in-memory only (no block allocator, no journal, no disk flush)
- **#3** GP fault during fork/clone (mitigated with RFLAGS sanitization)
- **#20** No performance tracing (ftrace, kprobes)
- **#22** No crash dump / reliability engineering
