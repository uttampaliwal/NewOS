# Contributing to turnix

Thank you for your interest in contributing to turnix! This document covers setup, standards, and workflow.

## Quick Start

```bash
git clone https://github.com/uttampaliwal/turnix.git
cd turnix
cargo xtask doctor          # verify environment
```

## Development Setup

### Prerequisites

- **Rust nightly** (via rustup)
- **QEMU** with OVMF UEFI firmware
- **Git**

### Platform-Specific

<details>
<summary><b>Linux (Ubuntu/Debian)</b></summary>

```bash
sudo apt-get install qemu-system-x86 edk2-ovmf
```
</details>

<details>
<summary><b>Linux (Arch)</b></summary>

```bash
sudo pacman -S qemu-full edk2-ovmf
```
</details>

<details>
<summary><b>macOS</b></summary>

```bash
brew install qemu
```
</details>

<details>
<summary><b>Windows</b></summary>

```powershell
choco install qemu
```
</details>

### Toolchain Setup

```bash
rustup install nightly
rustup default nightly
rustup target add x86_64-unknown-uefi x86_64-unknown-none --toolchain nightly
```

### Verify

```bash
cargo xtask doctor
```

## Build & Run

```bash
cargo xtask build-uefi       # UEFI loader
cargo xtask build-kernel     # freestanding kernel
cargo xtask run-uefi         # run in QEMU
```

## Before Submitting a PR

Run this checklist locally:

```bash
cargo fmt --check                          # formatting
RUSTFLAGS="-D warnings" cargo clippy      # zero warnings
cargo test --workspace                     # ~974 tests pass
cargo xtask build-uefi && cargo xtask build-kernel  # builds succeed
```

## Coding Standards

### Rust Style

- **Formatting**: `cargo fmt` (rustfmt, nightly)
- **Linting**: `clippy` with `-D warnings` — zero tolerance
- **no_std**: Kernel code uses `no_std` + `alloc`; never import `std`
- **Errors**: No `.unwrap()` in kernel code — use `match`, `ok_or`, or `expect` with context
- **Naming**: Follow Rust conventions (`snake_case` functions, `CamelCase` types)
- **Documentation**: Public items must have `///` doc comments

### Commit Messages

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]
```

**Types**: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`, `ci`

**Examples**:
```
feat(kernel): add eventfd system call
fix(scheduler): resolve CFS vruntime underflow
docs: update README with phases 12-13
chore: run cargo fmt
```

### Project Structure

```
kernel/         - no_std kernel core (ring 0)
boot/           - UEFI loader entry point
shared/         - crates shared between kernel and userland
  abi/          - syscall ABI types
  serial/       - serial port abstraction
  turnix-ipc-proto/  - IPC protocol definitions
  turnix-tpkg-format/ - package manifest format
userland/       - userspace services and applications
tools/xtask/    - build automation
fuzz/           - fuzz testing targets
docs/           - ADRs, architecture notes, phase documentation
.github/        - CI workflows and templates
```

## Fuzz Testing

Fuzz targets live in `fuzz/`. Run locally:

```bash
cd fuzz && cargo build --release
for target in fuzz_elf_parser fuzz_seccomp_bpf fuzz_ipc_message fuzz_vfs_path fuzz_syscall_args; do
    timeout 60 ./target/release/$target < /dev/urandom 2>/dev/null || true
done
```

Fuzz tests run automatically on `master` merges via CI.

## Security

If you discover a security vulnerability, see [SECURITY.md](SECURITY.md) for responsible disclosure instructions. **Do not** open a public issue for security vulnerabilities.

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating, you agree to uphold its standards.

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
