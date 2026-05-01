# Phase 2: Freestanding Handoff

## Goal

Load a separate `x86_64-unknown-none` kernel image from the UEFI loader, exit boot services, and jump into the kernel with a stable `BootInfo`.

## Why this matters

This is the point where turnix stops being a firmware-facing demo and becomes a real operating system bring-up effort. The loader still exists, but the kernel now owns execution after the firmware handoff.

## What is in place

- a freestanding kernel ELF image with a custom linker layout
- a UEFI loader that reads the kernel image from the boot volume
- exact-address loading of the kernel image into memory
- `ExitBootServices` before kernel transfer
- a `BootInfo` structure that carries memory-map metadata into the kernel
- serial output from the freestanding kernel stage

## Verified commands

- `cargo test`
- `cargo xtask doctor`
- `cargo xtask run-uefi`

## Verified output shape

The current serial log proves that:

- the UEFI loader starts
- the kernel file is loaded from disk
- boot services are exited
- the freestanding kernel runs and receives the boot contract

## Next step

Use the handed-off memory map to start real kernel bring-up:

- physical memory map parsing
- page-frame allocation
- page tables
- interrupt and timer initialization

This next step is now underway through the Phase 3 memory bring-up milestone.
