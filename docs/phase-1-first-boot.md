# Phase 1: First Boot Plan

This is the next concrete milestone after the current repository scaffold.

## Goal

Boot a tiny NewOS kernel in QEMU and print a reliable message over serial output.

## Why this matters

This is the first moment where the project becomes a real operating system effort rather than just a codebase. Once we can boot and log, every later subsystem becomes easier to debug.

## Expected deliverables

- Windows host instructions for installing QEMU
- Rust toolchain instructions for freestanding kernel builds
- bootloader integration using Limine
- kernel entry point and linker/layout decisions documented
- serial logging and panic output visible in QEMU
- one command to build and one command to run the image

## Implementation slices

1. Add the required toolchain pieces and document each install step.
2. Add a freestanding kernel target configuration.
3. Integrate Limine and generate a bootable image.
4. Replace the current kernel library skeleton with the first entry/boot path.
5. Add serial writer support and a minimal panic handler.
6. Add an `xtask` command that builds and runs QEMU consistently.

## Success criteria

- QEMU launches from the documented command on your Windows machine
- the kernel reaches its entry point
- serial output shows a deterministic boot message
- a forced panic prints something useful instead of hanging silently

## Non-goals

- user mode
- memory allocator
- interrupts
- filesystems
- GUI

