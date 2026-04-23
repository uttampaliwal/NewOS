# Windows Host Setup

This guide is for the current host machine: Windows 11 with Rust already installed.

## Already available

- `rustc`
- `cargo`
- `rustup`
- `git`
- stable toolchain: `stable-x86_64-pc-windows-msvc`

## Still needed for the first boot milestone

- QEMU for x86_64 emulation
- a freestanding Rust target and likely nightly Rust for low-level kernel builds
- Limine binaries or source checkout

## Checked on this machine

- `cargo test` succeeds for the current default workspace members
- `cargo check -p newos-kernel --lib` succeeds
- `qemu-system-x86_64` was not found on the current PATH

## Recommended host workflow

1. Keep Windows 11 as the main development host.
2. Build and test the early kernel in QEMU before touching real hardware.
3. Use the external HDD for VM images, backups, and artifacts.
4. Buy a separate SSD later for bare-metal testing.

## Bare-metal safety rules

- Back up BitLocker recovery information before changing boot or firmware settings.
- Do not test early kernels on the main Windows drive.
- Prefer integrated graphics and Ethernet before chasing dedicated GPU or Wi-Fi support.
- Treat the 32 GB pendrive as installer/rescue media only.

## Planned future additions

- exact QEMU install steps for Windows
- exact Rust target install commands
- PowerShell helpers for build/run/debug
