# NewOS

**A Rust-first operating system built step by step for learning and long-term usability.**

NewOS aims to feel familiar to Linux users while keeping a cleaner internal design. It is not a Linux clone and not a distro. It is a new operating system with Linux-like workability and a modern desktop roadmap.

## Current Status

| Phase | Status | Milestone |
|-------|--------|-----------|
| 0 | Complete | Foundation and scaffold |
| 1 | Complete | First boot and UEFI loader |
| 2 | Complete | Freestanding kernel handoff |
| 3 | Complete | Physical memory bring-up |
| 4 | Complete | Stable kernel scheduler baseline |
| 5 | In progress | User mode and ELF runtime |
| 6 | Pending | Terminal-first usability |
| 7 | Pending | Wayland desktop path |

## What Works

- Verified UEFI loader to freestanding kernel handoff in QEMU
- `x86_64-unknown-none` kernel image with serial output
- Physical frame allocator and kernel heap bring-up
- GDT, IDT, TSS, and LAPIC timer initialization
- Stable higher-half kernel task scheduling baseline
- **Initramfs support** (ABI v3) and global VFS
- **Advanced ELF Loader** with segment mapping and BSS support

## Current Architectural Position

The kernel now prioritizes a dependable higher-half bring-up path:

- UEFI loader stages a freestanding kernel ELF
- the loader exits boot services and passes an explicit `BootInfo`
- the kernel initializes paging, heap, GDT, IDT, TSS, and timer interrupts
- execution starts from real kernel tasks with mapped kernel stacks

Early user mode is intentionally deferred until the next execution phase. The previous experiment of copying kernel Rust function bytes into user pages is not a sound user-program model, so the project now treats `kernel threads first, user mode later` as the correct milestone boundary.

## Quick Start

```powershell
cargo xtask doctor
cargo xtask run-uefi

# Compatibility alias
cargo xtask uefi-loader
```

See [docs/quickstart.md](C:\Users\uttam\development\NewOS\docs\quickstart.md) for setup details.

## Documentation

- [Architecture](C:\Users\uttam\development\NewOS\docs\architecture.md)
- [Roadmap](C:\Users\uttam\development\NewOS\docs\roadmap.md)
- [Phase 1 First Boot](C:\Users\uttam\development\NewOS\docs\phase-1-first-boot.md)
- [Phase 2 Freestanding Handoff](C:\Users\uttam\development\NewOS\docs\phase-2-freestanding-handoff.md)
- [Phase 3 Physical Memory Bring-Up](C:\Users\uttam\development\NewOS\docs\phase-3-memory-bringup.md)
- [Phase 4 Stable Kernel Tasks](C:\Users\uttam\development\NewOS\docs\phase-4-stable-kernel-tasks.md)
- [Windows Host Setup](C:\Users\uttam\development\NewOS\docs\windows-host-setup.md)

## Repository Layout

```text
docs/          project documentation, ADRs, and guides
boot/          UEFI firmware-facing entry points
kernel/        freestanding kernel core
shared/abi     shared types for kernel or future userland boundaries
shared/serial  low-level serial output support
tools/xtask    developer automation
```

## Principles

- Learn deeply while building something real
- Prefer clean interfaces over milestone shortcuts
- Use open standards where compatibility matters
- Keep unsafe Rust small and justified
- Write docs as we go so the architecture stays replaceable
