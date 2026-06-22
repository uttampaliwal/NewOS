# Testing

Turnix has a layered test strategy covering host-side logic, kernel internals, and
full-system QEMU boot tests.

## Host Tests

Run unit tests on shared crates (ABI, serial, error types) that compile for the host:

```bash
cargo test --all-targets
```

## Kernel Tests

Target-specific property tests for memory management and security models using the
`proptest` crate:

```bash
cargo test -p turnix-kernel
```

## QEMU Smoke Tests

Full boot and userland smoke tests run the kernel inside QEMU headless. These verify
UEFI loader handoff, kernel initialization, VFS mounts, and process execution:

```bash
TURNIX_QEMU_DISPLAY=none cargo xtask test-qemu
```

## Test Organization

- `shared/abi/` — ABI encoding/decoding unit tests
- `kernel/` — property-based tests for VMAs, page tables, OOM scoring
- `tools/xtask/` — integration harness driving QEMU
- `userland/` — individual userland binaries serve as integration test subjects
