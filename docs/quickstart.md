# Quickstart

This repository is intentionally organized for learning. Start here before touching the kernel code.

## Read first

1. [Project Overview](C:\Users\uttam\development\NewOS\README.md)
2. [Architecture](C:\Users\uttam\development\NewOS\docs\architecture.md)
3. [Roadmap](C:\Users\uttam\development\NewOS\docs\roadmap.md)
4. [Windows Host Setup](C:\Users\uttam\development\NewOS\docs\windows-host-setup.md)
5. [Phase 1 First Boot Plan](C:\Users\uttam\development\NewOS\docs\phase-1-first-boot.md)

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
- only the stable Windows Rust toolchain is installed right now
- `qemu-system-x86_64` is not currently installed or on the path
