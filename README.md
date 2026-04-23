# NewOS

NewOS is a Rust-first operating system project built step by step for learning and long-term usability.

The goal is a secure, modern, Linux-like operating system with a terminal-first first release and a Wayland-based GUI later. We are building it in a way that keeps each subsystem understandable, replaceable, and well documented.

## Current phase

We have completed `Phase 0`, completed the first UEFI boot milestone, and started `Phase 2`: freestanding kernel handoff.

## Repository layout

- `docs/` project docs, architecture notes, milestone plans, and host setup guides
- `boot/` early boot and firmware-facing entrypoints
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
- kernel crate skeleton and kernel architecture notes
- `xtask` developer commands for host checks and staged boot artifacts
- a verified UEFI loader path that boots in QEMU and prints over serial
- a verified `x86_64-unknown-none` freestanding kernel image
- a verified `UEFI loader -> ExitBootServices -> freestanding kernel` handoff

## Immediate next milestones

1. Build and run the UEFI first-boot path under QEMU with serial output
2. Extend the freestanding kernel entry into real memory-management bring-up
3. Add paging, interrupts, and a basic memory allocator
4. Introduce the first user/kernel ABI boundaries

Start with [docs/quickstart.md](C:\Users\uttam\development\NewOS\docs\quickstart.md).

## Useful commands

- `cargo xtask doctor`
- `cargo xtask build-uefi`
- `cargo xtask run-uefi`
