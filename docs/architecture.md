# Architecture

## Product direction

NewOS aims to feel familiar to Linux users while keeping a cleaner internal design. It is not a Linux clone and not a distro. It is a new operating system with Linux-like workability.

## Core architectural choices

- `Kernel model`: Rust-first modular monolith
- `Primary platform`: x86_64 on QEMU first, then one reference real machine
- `Boot strategy`: UEFI-first bring-up with a thin loader, then freestanding kernel handoff
- `Early UX`: terminal-first
- `Future desktop`: Wayland-oriented compositor and desktop stack
- `Compatibility`: Linux-like behavior and strong source portability, not Linux binary compatibility as an early target
- `Security`: secure-by-default, least privilege, signed artifacts, rollback-friendly system design

## Layer model

### 1. Firmware and boot

The first bring-up step uses a thin UEFI loader because it matches modern hardware, works well on the current Windows and QEMU host, and keeps the early learning loop short. This loader is a staging point, not the long-term kernel architecture.

We have now crossed the first real boundary: the UEFI loader stages a separate freestanding kernel image, exits boot services, and jumps into the kernel with an explicit `BootInfo` contract.

The next job is to deepen that freestanding kernel runtime with memory-management and interrupt setup instead of adding more firmware-side complexity.

### 2. Kernel core

The kernel owns:

- CPU initialization
- interrupts and timers
- physical and virtual memory management
- task scheduling
- syscall dispatch
- core device and filesystem abstractions

Even in the current UEFI-first milestone, we keep a visible handoff boundary between loader-facing code and kernel-facing code. That habit will make the later freestanding transition much cleaner.

The current execution baseline is intentionally conservative: `kernel threads first, user mode later`. We still want user processes, ELF loading, and a Linux-like userspace model, but we are only reintroducing them after the higher-half kernel, interrupt model, stack discipline, and scheduler frame layout are stable.

### 3. System services

Long term, more policy should live outside the kernel than inside it. The kernel should provide mechanisms; higher-level services should provide user-facing behavior where possible.

### 4. Userland

Userland begins with an `init` process, a shell, basic utilities, and a native libc/runtime boundary. We then grow toward a Linux-like application environment and later a graphical desktop.

## Design rules

- Keep unsafe Rust small and justified.
- Favor explicit traits and typed handles over global state.
- Make subsystems observable with logs, metrics, and error enums.
- Prefer standards over custom formats unless we have a strong reason otherwise.
- Document the reason for each non-obvious architectural choice.
