# Roadmap

## Completed Phases

| Phase | Milestone | Status |
|-------|-----------|--------|
| 1 | UEFI Boot, ACPI, PCIe, VirtIO-Net, NVMe, XHCI, DRM/KMS | Complete |
| 2 | VMAs, Demand Paging, mmap/munmap, Page Cache, Swap, ASLR, OOM | Complete |
| 3 | Process table, fork/exec/wait, VFS (tmpfs/ext4), Pipes, Sockets, Signals | Complete |
| 4 | POSIX Capabilities, Namespaces, Seccomp-BPF, LSM hooks, IMA/EVM | Complete |
| 5 | Package manager, SAT solver, TUF verification, staging/rollback | Complete |
| 6 | IPC Broker, structured logging, service unit manager | Complete |
| 7 | Wayland compositor, input routing, desktop session management | Complete |

## Future Work

- **SMP (Symmetric Multi-Processing)**: Per-CPU schedulers, lock-free data structures,
  and inter-processor interrupts for multi-core support.
- **Networking Stack**: TCP/IP implementation using `smoltcp`, VirtIO-Net driver
  integration, and socket API for userland applications.
- **Self-Hosting**: Compile the Turnix kernel and userland from within Turnix itself.
  Requires a working C/Rust toolchain, make, and linker running on the OS.
- **POSIX Compliance**: Expand syscall coverage for `poll`/`epoll`, `mmap` flags,
  and additional signal semantics.
- **Power Management**: Suspend/resume, CPU frequency scaling, and thermal management
  via ACPI.
