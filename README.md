# NewOS

NewOS is a Rust-first operating system project built step by step for learning and long-term usability.

The goal is a secure, modern, Linux-like operating system with a terminal-first first release and a Wayland-based GUI later. We are building it in a way that keeps each subsystem understandable, replaceable, and well documented.

## Current phase

We are in `Phase 0`: workspace setup, documentation, tooling, and the first kernel skeleton.

## Repository layout

- `docs/` project docs, architecture notes, milestone plans, and host setup guides
- `kernel/` the future kernel crate and low-level boot/runtime materials
- `shared/` shared interfaces and types that can be reused across kernel and userland
- `tools/xtask/` developer automation entrypoints for building, testing, and packaging

## Principles

- Learn deeply while building something real
- Prefer modern, maintainable designs over clever shortcuts
- Use open standards where compatibility matters
- Keep interfaces explicit so parts can be upgraded or replaced cleanly
- Write docs as we go so future changes stay understandable

## What works today

- Rust workspace scaffold
- shared ABI crate with host-testable types
- `xtask` command skeleton for future developer workflows
- kernel crate skeleton and kernel architecture notes

## Immediate next milestones

1. Install the missing emulator/toolchain pieces on the Windows host
2. Make the kernel crate boot under QEMU with serial output
3. Add paging, interrupts, and a basic memory allocator
4. Introduce the first user/kernel ABI boundaries

Start with [docs/quickstart.md](C:\Users\uttam\development\NewOS\docs\quickstart.md).

