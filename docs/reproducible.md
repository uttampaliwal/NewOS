# Reproducible Builds for Turnix OS

This document provides instructions for building Turnix OS in a reproducible manner.

## Overview

Turnix OS uses pinned toolchain versions and deterministic build settings to ensure reproducible artifacts across different environments.

## Build Requirements

- Rust: `stable-2024-12-20` (pinned in `rust-toolchain.toml`)
- Cargo: Latest stable
- QEMU: For testing (minimum v7.0)
- xorriso: For ISO creation (optional)

## Build Process

### 1. Install Toolchain

```bash
rustup toolchain install stable-2024-12-20
rustup component add rustfmt clippy
rustup target add x86_64-unknown-none
```

### 2. Build Deterministically

```bash
# Set reproducible build environment
export SOURCE_DATE_EPOCH=$(date +%s)

# Build workspace
cargo build --workspace --release

# Verify reproducibility
cargo fmt -- --check
cargo clippy -- -D warnings
```

### 3. Test with QEMU

```bash
# Run smoke test
./scripts/qemu-smoke.sh

# Run full test suite
cargo xtask test-qemu
```

## Build Artifacts

After a successful build, you'll find:

- `target/x86_64-unknown-none/release/turnix-kernel` - Kernel binary
- `target/x86_64-unknown-uefi/release/turnix-uefi-loader.efi` - UEFI bootloader
- `out/turnix.iso` - Bootable ISO image (when using `make iso`)

## Verification

To verify your build matches the expected artifacts:

```bash
# Check binary signatures
sha256sum target/x86_64-unknown-none/release/turnix-kernel

# Compare with reference hashes (when available)
# sha256sum --check turnix-kernel.sha256
```

## Continuous Integration

The CI pipeline automatically runs reproducibility checks:

- Format checks (`cargo fmt -- --check`)
- Linting (`cargo clippy -- -D warnings`)
- Build verification (`cargo build --workspace --release`)
- QEMU smoke tests

## Troubleshooting

### Build Failures

1. **Toolchain version mismatch**: Ensure you're using the exact pinned version
2. **Missing dependencies**: Run `cargo build --workspace` to fetch all dependencies
3. **Target not found**: Add target with `rustup target add x86_64-unknown-none`

### Test Failures

1. **QEMU not found**: Install QEMU (`sudo apt install qemu-system-x86`)
2. **Permission issues**: Ensure scripts are executable (`chmod +x scripts/qemu-smoke.sh`)
3. **Kernel panics**: Check serial output in `qemu-serial.log`

## Contributing

When submitting changes:

1. Always run `cargo fmt` and `cargo clippy` locally
2. Ensure CI passes on your branch
3. Update this document if build processes change
4. Test with QEMU before submitting PRs

## Security Considerations

- Build artifacts should be signed before distribution
- Verify source code integrity before building
- Use reproducible builds for security auditing