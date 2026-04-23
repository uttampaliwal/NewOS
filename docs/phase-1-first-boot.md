# Phase 1: First Boot Plan

This is the next concrete milestone after the current repository scaffold.

## Goal

Boot a tiny NewOS UEFI loader in QEMU and print a reliable message over serial output.

## Why this matters

This is the first moment where the project becomes a real operating system effort rather than just a codebase. Once we can boot and log, every later subsystem becomes easier to debug.

We are deliberately using a thin UEFI-first bring-up step because it is the lowest-friction path on the current Windows and QEMU host. The freestanding kernel target is already prepared for the next phase.

## Expected deliverables

- UEFI loader crate
- documented QEMU plus EDK2 run workflow
- serial logging and a useful panic path
- one command to build and one command to run the image
- documented transition plan toward a freestanding kernel handoff
- serial logging and panic output visible in QEMU

## Implementation slices

1. Verify the required host toolchain pieces and document the actual machine state.
2. Add a UEFI loader crate targeting `x86_64-unknown-uefi`.
3. Add serial writer support and a minimal panic handler.
4. Add an `xtask` command that builds the EFI image and stages it into an EFI system partition directory.
5. Add an `xtask` command that runs QEMU with EDK2 and a virtual FAT disk.
6. Keep the freestanding `x86_64-unknown-none` target ready for the next phase.

## Success criteria

- QEMU launches from the documented command on your Windows machine
- the loader reaches its entry point
- serial output shows a deterministic boot message
- a forced panic prints something useful instead of hanging silently
- the build and run flow is simple enough to repeat while learning

## Current status

The serial success path has been verified on the current Windows 11 host using:

- `cargo xtask doctor`
- `cargo xtask run-uefi`

The verified output now comes from the kernel stage through an explicit boot contract, not only from the firmware-facing loader.

This phase is now complete and has been followed by a separate freestanding kernel handoff phase.

## Non-goals

- user mode
- full kernel handoff
- memory allocator
- interrupts
- filesystems
- GUI

## Next step after this phase

Move the booted path from a firmware-facing loader into a freestanding kernel entry with explicit handoff boundaries.
