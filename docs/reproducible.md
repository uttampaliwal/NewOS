# Reproducible Builds for Turnix OS

This document provides instructions for building Turnix OS in a reproducible manner.

## Overview

Turnix OS uses nightly Rust toolchain and deterministic build settings to ensure reproducible artifacts across different environments.

## Build Requirements

- Rust: nightly (via `rust-toolchain.toml`)
- QEMU: For testing (minimum v7.0)

## Build Process

### 1. Install Toolchain

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustup target add x86_64-unknown-uefi
rustup target add x86_64-unknown-none
```

### 2. Build

```bash
# Build workspace
cargo build --workspace

# Verify
cargo fmt -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace
```

### 3. Test with QEMU

```bash
# Run all CI test suites
cargo xtask ci-boot
cargo xtask ci-test
cargo xtask ci-driver-tests
cargo xtask ci-security
cargo xtask ci-bench
```

## Build Artifacts

After a successful build, you'll find:

- `target/x86_64-unknown-none/release/turnix-kernel` - Kernel binary
- `target/x86_64-unknown-uefi/release/turnix-uefi-loader.efi` - UEFI bootloader

## Continuous Integration

The CI pipeline automatically runs:

- Format checks (`cargo fmt -- --check`)
- Linting (`RUSTFLAGS="-D warnings" cargo clippy --workspace`)
- Build verification (`cargo build --workspace`)
- QEMU boot gate tests (30 consecutive boots)
- Driver detection tests
- Security subsystem tests
- Benchmark validation

## Troubleshooting

### Build Failures

1. **Toolchain mismatch**: Ensure you're using nightly via `rust-toolchain.toml`
2. **Missing targets**: Run `rustup target add x86_64-unknown-uefi x86_64-unknown-none`

### Test Failures

1. **QEMU not found**: Install QEMU (`sudo apt install qemu-system-x86`)
2. **OVMF missing**: Set `TURNIX_OVMF_CODE` and `TURNIX_OVMF_VARS` environment variables
3. **Kernel panics**: Check serial output in `out/ci-boot.log`

## Contributing

When submitting changes:

1. Always run `cargo fmt` and `RUSTFLAGS="-D warnings" cargo clippy --workspace` locally
2. Ensure CI passes on your branch
3. Test with QEMU: `cargo xtask ci-boot`
