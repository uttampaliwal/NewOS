# Windows Host Setup

This guide is for the current host machine: Windows 11 with Rust already installed.

## Already available

- `rustc`
- `cargo`
- `rustup`
- `git`
- stable toolchain: `stable-x86_64-pc-windows-msvc`
- nightly toolchain: `nightly-x86_64-pc-windows-msvc`
- QEMU 11 on the path

## Still needed for the first boot milestone

- build verification of the new UEFI boot path
- freestanding kernel handoff after the UEFI step

## Checked on this machine

- `cargo test` succeeds for the current default workspace members
- `cargo check -p newos-kernel --lib` succeeds
- `qemu-system-x86_64` is on the current PATH
- QEMU was installed under `C:\msys64\ucrt64\bin`
- EDK2 firmware was found at `C:\msys64\ucrt64\share\qemu\edk2-x86_64-code.fd`
- EDK2 vars storage was found at `C:\msys64\ucrt64\share\qemu\edk2-i386-vars.fd`
- nightly Rust is installed and up to date as of this setup pass
- nightly components include `rust-src`, `rustfmt`, `clippy`, and `llvm-tools-preview`
- nightly targets now include `x86_64-unknown-uefi` and `x86_64-unknown-none`
- `cargo xtask run-uefi` succeeded on this machine

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

- PowerShell helpers for build/run/debug
- bare-metal bring-up guide for the separate SSD path

## Useful environment overrides

- `NEWOS_OVMF_CODE` override the firmware code image path
- `NEWOS_OVMF_VARS` override the firmware vars image path
- `NEWOS_QEMU_ACCEL` override the accelerator, for example `whpx` or `tcg`
