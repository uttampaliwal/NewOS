# Kernel/User Boundary

Turnix is described as a "microkernel/modular monolith hybrid." This document
defines what runs where.

## What Runs in Kernel Space

| Component | Why |
|-----------|-----|
| Scheduler | Context switches require CR3/TSS access |
| VMM | Page table manipulation |
| VFS | Filesystem mount tables and inode management |
| IPC (pipes, sockets) | Cross-process data flow with kernel buffering |
| Security hooks | Capability checks, LSM evaluation, seccomp BPF |
| Syscall handler | Ring 3 -> Ring 0 transition |
| Device drivers | Direct hardware access (PCIe MMIO, port I/O) |
| ELF loader | Binary loading with ASLR |
| Network stack | smoltcp TCP/IP with VirtIO-Net |

## What Runs in User Space

| Component | Why |
|-----------|-----|
| init daemon | PID 1, service management |
| shell | Interactive command line |
| compositor | GPU rendering (DRM/KMS) |
| display-manager | Login and session management |
| ipc-broker | Service routing |
| log-daemon | Log collection and rotation |
| network-manager | DHCP, DNS, static config |
| device-manager | Hotplug event handling |
| package-manager | tpkg install/remove/upgrade |
| desktop-shell | Taskbar, app launcher |

## IPC Surface

Userland communicates with the kernel via:

1. **System calls** — 87 syscalls via `syscall` instruction
2. **Shared memory** — `mmap` for framebuffer and IPC buffers
3. **Device files** — `/dev/` entries for block/char devices

## Potential User-Space Migrations

These components could be moved to user space in the future:

| Component | Migration Benefit | Complexity |
|-----------|------------------|------------|
| Network stack | Better isolation, restartability | High |
| Device manager | Hotplug without kernel risk | Medium |
| Log daemon | Already user-space | Done |
| Filesystem drivers | Crash isolation | High |

## TCB (Trusted Computing Base)

The TCB includes everything that must be correct for security:

- Scheduler (~200 lines unsafe)
- VMM (~400 lines unsafe)
- Syscall handler (~1500 lines)
- Security framework (~800 lines)
- IPC (~600 lines)
- ELF loader (~300 lines)
- Boot handoff (~500 lines)

**Out of TCB:** All userland processes, device drivers (trait-bounded).
