# Quickstart Guide

This guide describes how to set up, build, run, and verify the Turnix operating system workspace.

---

## 🛠️ Environment Prerequisites

Turnix requires a **nightly Rust** toolchain, **QEMU**, and **EDK2 UEFI** firmware.

To verify your environment's compatibility, run:
```bash
cargo fmt --check
cargo xtask doctor
```

### 1. OVMF Firmware Setup
UEFI firmware paths vary depending on the host distribution. If `cargo xtask doctor` reports missing firmware, set the corresponding environment variables in your shell (e.g. for Arch Linux):
```bash
export TURNIX_OVMF_CODE="/usr/share/edk2/x64/OVMF_CODE.4m.fd"
export TURNIX_OVMF_VARS="/usr/share/edk2/x64/OVMF_VARS.4m.fd"
```

---

## 🚀 Running Turnix

### 1. Interactive Boot (with QEMU display)
Build the loaders, freestanding kernel, ramdisk, and run them inside QEMU:
```bash
cargo xtask run-uefi
```

### 2. Headless Boot (serial output only)
To run the boot sequence headlessly (e.g., in a server environment or CI runner):
```bash
TURNIX_QEMU_DISPLAY=none cargo xtask test-qemu
```
*Note: In headless mode, the scheduler will run the kernel worker tasks (printing `w` continuously) and execute the user space init daemon.*

---

## 🧪 Testing & Verification

Turnix includes a multi-layered testing workflow to verify changes and prevent regressions:

### 1. Host Workspace Tests
Verifies platform-independent workspace members, init manifest parsers, and system ABI data structures:
```bash
cargo test --all-targets
```

### 2. Kernel-Specific Tests
Verifies the higher-half memory mappings, demand paging invariants, VMAs, POSIX capability sets, BPF seccomp instruction sets, and Unix socket data integrity on the host:
```bash
cargo test -p turnix-kernel
```

### 3. UEFI / QEMU Integration Smoke Tests
Launches QEMU in headless mode, builds the UEFI loader, and verifies the full guest OS boot loop up to scheduling user space tasks:
```bash
TURNIX_QEMU_DISPLAY=none cargo xtask test-qemu
```

---

## 📝 Development Workflow

To submit changes to the codebase, please follow these steps:
1. Run local tests: `cargo test --all-targets` and `cargo test -p turnix-kernel`.
2. Format code and run check style: `cargo fmt` and `cargo clippy --workspace --all-targets`.
3. Verify QEMU boots: `cargo xtask test-qemu`.
4. Submit your pull request to the `turnix-next` integration branch.
