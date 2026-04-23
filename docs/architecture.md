# Architecture

## Product direction

NewOS aims to feel familiar to Linux users while keeping a cleaner internal design. It is not a Linux clone and not a distro. It is a new operating system with Linux-like workability.

## Core architectural choices

- `Kernel model`: Rust-first modular monolith
- `Primary platform`: x86_64 on QEMU first, then one reference real machine
- `Boot strategy`: UEFI plus Limine
- `Early UX`: terminal-first
- `Future desktop`: Wayland-oriented compositor and desktop stack
- `Compatibility`: Linux-like behavior and strong source portability, not Linux binary compatibility as an early target
- `Security`: secure-by-default, least privilege, signed artifacts, rollback-friendly system design

## Layer model

### 1. Firmware and boot

UEFI firmware loads a bootloader that hands control to our kernel using a stable boot protocol. We will rely on a mature bootloader first so we can spend our learning time on kernel design rather than bootloader maintenance.

### 2. Kernel core

The kernel owns:

- CPU initialization
- interrupts and timers
- physical and virtual memory management
- task scheduling
- syscall dispatch
- core device and filesystem abstractions

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

