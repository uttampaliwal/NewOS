# Architecture Overview

Turnix is organized into four main vertical layers:

```
+-----------------------------------------------------------+
|                        USER SPACE                         |
|   init daemon | interactive shell | fault-tester | apps   |
+-----------------------------------------------------------+
                             |
                   System Call Boundary
                             |
+-----------------------------------------------------------+
|                       KERNEL CORE                         |
|   Scheduler   |   VFS (Mounts)   |  VMM (Demand Paging)   |
|   Signals     |   Pipes & IPC    |  Capabilities / LSM    |
+-----------------------------------------------------------+
                             |
+-----------------------------------------------------------+
|                     DRIVER FRAMEWORK                      |
|   PCIe ECAM   |   ACPI AML Evaluator   |  DRM Framebuffer |
|   VirtIO-Net  |   NVMe Controller      |  XHCI USB Host   |
+-----------------------------------------------------------+
                             |
+-----------------------------------------------------------+
|                     FIRMWARE & BOOT                       |
|          UEFI Loader  <--->  EDK2 / OVMF Firmware         |
+-----------------------------------------------------------+
```

## Key Design Principles

1. **Memory Safety** — Unsafe Rust is constrained to hardware registers, page tables, and context switching. Every `unsafe` block has a `// SAFETY:` comment.

2. **Concurrency Safety** — Synchronization via `spin::Mutex`. Lock ordering rules prevent deadlocks.

3. **Minimal TCB** — The Trusted Computing Base includes scheduler, VMM, syscall handler, and security framework. Drivers and userland are outside the TCB.

4. **Pluggable Security** — LSM hooks, seccomp-BPF filters, and POSIX capabilities are composable and enforce least-privilege.

## Subsystem Map

| Subsystem | Location | Purpose |
|-----------|----------|---------|
| Scheduler | `kernel/src/task/` | Round-robin preemptive scheduling |
| VMM | `kernel/src/memory/` | Demand paging, ASLR, W^X |
| VFS | `kernel/src/fs/` | Mount tables, tmpfs, ext4 |
| IPC | `kernel/src/ipc/` | Pipes, Unix domain sockets |
| Security | `kernel/src/security/` | Capabilities, namespaces, seccomp, LSM, IMA/EVM |
| Drivers | `kernel/src/drivers/` | NVMe, VirtIO-Net, XHCI USB, DRM/KMS |
| Syscalls | `kernel/src/syscall/` | Ring 3 -> Ring 0 transition |
