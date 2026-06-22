# Kernel

The `kernel` crate is the core of Turnix OS — a no_std, higher-half, x86_64 monolithic kernel.

## Modules

| Module | Description |
|--------|-------------|
| `acpi` | ACPI table parsing (RSDP, XSDT, MCFG, DSDT/SSDT) and AML interpreter |
| `arch` | Architecture-specific code (GDT, IDT, TSS, context switching) |
| `block` | Block device I/O layer (`BlockDevice` trait, block cache) |
| `boot` | Early boot initialization, driver probing, stage tracking |
| `drivers` | Device drivers: PCI/PCIe, VirtIO-Net, NVMe, XHCI USB, GPU/DRM, TPM |
| `elf` | ELF binary loader for userspace processes |
| `fs` | Virtual File System (VFS), tmpfs, ext4 (read-only), mount management |
| `ipc` | Unix domain sockets and ring-buffered pipes |
| `log_ring` | Kernel log ring buffer with dmesg syscall |
| `memory` | VMM, page tables, heap allocator, page cache, swap, OOM killer |
| `net` | smoltcp-based TCP/IP stack, socket syscalls, network interface config |
| `process` | Process table, fork/exec/wait, signal handling, file descriptor tables |
| `security` | Capabilities, namespaces, seccomp-BPF, LSM hooks, IMA/EVM |
| `serial` | Serial port (COM1) output for kernel logging |
| `smp` | Symmetric multiprocessing (AP bring-up) |
| `syscall` | System call dispatch and handler |
| `task` | Task structures, preemptive round-robin scheduler |
| `time` | Monotonic clock (uptime in microseconds) |
| `tty` | Terminal I/O |

## Testing

```bash
cargo test -p turnix-kernel
```

## Known Limitations

See [KNOWN_ISSUES.md](../KNOWN_ISSUES.md) for current limitations including:
- ext4 writes delegate to tmpfs (no journaling)
- EVM HMAC key is hardcoded
- GP fault during fork/clone
