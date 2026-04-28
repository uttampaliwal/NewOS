# Quickstart

This repository is intentionally organized for learning. Start here before touching the kernel code.

## Read first

1. [Project Overview](../README.md)
2. [Architecture](architecture.md)
3. [Roadmap](roadmap.md)
4. [Windows Host Setup](windows-host-setup.md)
5. [Phase 1 First Boot Plan](phase-1-first-boot.md)
6. [ADR 0002 UEFI-First Bring-Up](adr-0002-uefi-first-bringup.md)
7. [Phase 2 Freestanding Handoff](phase-2-freestanding-handoff.md)
8. [Phase 3 Physical Memory Bring-Up](phase-3-memory-bringup.md)

## Development rhythm

For each milestone we will keep the same flow:

1. Write or update the design note.
2. Implement the smallest useful slice.
3. Verify it locally.
4. Record what we learned and what changes next.

## Phase 0 outcome

Phase 0 is complete when:

- the repository structure is stable
- the host setup guide is clear
- shared crates compile on the Windows host
- the kernel crate shape is ready for the first boot milestone

## Verified on the current machine

- `cargo test` passes for the host-buildable workspace members
- `cargo xtask status` runs successfully
- `cargo check -p newos-kernel --lib` succeeds
- QEMU 11 is installed and on the path
- nightly Rust is installed and updated
- the `x86_64-unknown-uefi` and `x86_64-unknown-none` targets are installed for nightly
- the EDK2 UEFI firmware image is available through the QEMU install
- `cargo xtask run-uefi` successfully reaches the freestanding kernel path in QEMU
- the freestanding kernel can summarize conventional memory and allocate sample physical frames
- the kernel can start its stabilized higher-half scheduler baseline with multiple kernel tasks

## Next practical step

Start with:

1. `cargo xtask doctor`
2. `cargo xtask run-uefi`
3. `cargo xtask uefi-loader` also works as a compatibility alias if that is the command name you already learned

If your firmware files live somewhere unusual, set:

- `NEWOS_OVMF_CODE`
- `NEWOS_OVMF_VARS`
- `NEWOS_QEMU_ACCEL`
