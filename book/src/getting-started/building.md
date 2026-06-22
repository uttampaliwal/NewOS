# Building from Source

## Toolchain

Turnix requires Rust nightly. The toolchain is pinned in `rust-toolchain.toml`:

```toml
[toolchain]
channel = "nightly-2026-06-22"
targets = ["x86_64-unknown-uefi", "x86_64-unknown-none"]
```

## Build Commands

```bash
# Kernel only
cargo build -p kernel

# All workspace crates
cargo build

# Release build
cargo build --release

# Specific userland binary
cargo build -p shell
cargo build -p init
```

## Host-Side Tools

```bash
# Build automation
cargo build -p xtask

# Benchmarks (runs on host)
cargo build -p host-benchmarks
cargo run -p host-benchmarks --release

# Fuzz targets
cd fuzz && cargo build --release
```

## Cross-Compilation

The kernel targets `x86_64-unknown-none` (freestanding). Userland binaries
are built as `no_std` executables. The UEFI loader targets `x86_64-unknown-uefi`.

All targets are managed automatically by the workspace Cargo configuration.
