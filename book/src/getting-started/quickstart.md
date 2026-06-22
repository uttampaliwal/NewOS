# Quickstart Guide

## Prerequisites

- Rust nightly toolchain (pinned to `nightly-2026-06-22`)
- QEMU with x86_64 support
- Make (optional, for convenience targets)

## Build

```bash
# Build the kernel
cargo build

# Build with optimizations
cargo build --release

# Run all tests
cargo test --workspace

# Run clippy lints
RUSTFLAGS="-D warnings" cargo clippy
```

## Run in QEMU

```bash
# Using xtask
cargo xtask run

# Without display
TURNIX_QEMU_DISPLAY=none cargo xtask run
```

## Run Tests in QEMU

```bash
# Boot and smoke test
cargo xtask test-qemu

# Full test suite
TURNIX_QEMU_DISPLAY=none cargo xtask test-qemu
```

## Project Structure

```
turnix/
├── boot/           # UEFI bootloader
├── kernel/         # Core kernel (scheduler, VMM, VFS, security)
├── shared/         # Shared crates (ABI, serial, IPC, error types)
├── userland/       # User-space programs (init, shell, compositor, daemons)
├── tools/          # Build automation (xtask)
├── docs/           # Documentation and ADRs
├── fuzz/           # Fuzz testing targets
└── benchmarks/     # Performance benchmarks
```
