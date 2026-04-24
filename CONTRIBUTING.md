# Contributing to NewOS

Thank you for your interest in contributing to NewOS! This document outlines how to set up your development environment, coding standards, and the contribution workflow.

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/<your-username>/NewOS.git`
3. Add the upstream remote: `git remote add upstream https://github.com/uttampaliwal/NewOS.git`
4. Create a feature branch: `git checkout -b feature/my-feature`

## Development Environment

### Prerequisites

- Rust nightly (via rustup)
- QEMU (x86_64 with OVMF)
- Platform-specific requirements below

### Windows Setup

1. Install [Rust nightly](https://rustup.rs):

```powershell
rustup install nightly
rustup default nightly
rustup target add x86_64-unknown-uefi --toolchain nightly
rustup target add x86_64-unknown-none --toolchain nightly
```

2. Install QEMU and OVMF:

```powershell
choco install qemu
```

### Linux Setup (Ubuntu/Debian)

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustup install nightly
rustup default nightly
rustup target add x86_64-unknown-uefi --toolchain nightly
rustup target add x86_64-unknown-none --toolchain nightly

# Install QEMU and OVMF
sudo apt-get update
sudo apt-get install qemu-system-x86 edk2-ovmf
```

### macOS Setup

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustup install nightly
rustup default nightly
rustup target add x86_64-unknown-uefi --toolchain nightly
rustup target add x86_64-unknown-none --toolchain nightly

# Install QEMU (via Homebrew)
brew install qemu
```

### Verify Setup

```powershell
cargo xtask doctor
```

This checks:
- Rust toolchain and targets (`x86_64-unknown-uefi`, `x86_64-unknown-none`)
- QEMU installation
- Required Cargo commands

### Build Commands

```powershell
# Build the UEFI loader
cargo xtask build-uefi

# Build the freestanding kernel
cargo xtask build-kernel

# Run in QEMU
cargo xtask run-uefi
```

## Coding Style

### General Principles

- Write clear, readable code over clever code
- Document **why**, not just **what**
- Keep functions small and focused
- Use meaningful names for types, functions, and variables

### Rust-Specific

- Follow the standard Rust fmt style (run `cargo fmt` before committing)
- Use `clippy` to catch common mistakes: `cargo clippy -- -D warnings`
- Prefer explicit type annotations in public APIs
- Use `#[must_use]` for functions that return important values
- Handle errors explicitly; avoid `.unwrap()` in kernel code

### No Std

- Kernel code uses `no_std`; do not import `std`
- Use `alloc` for heap-allocated types
- Use `core` for primitive operations
- Test host-buildable crates separately: `cargo test -p newos-abi`

### Commit Messages

- Use imperative mood: "Add feature" not "Added feature" or "Adds feature"
- Keep the subject line under 72 characters
- Reference issues: "Fixes #123" or "Closes #456"

Example:
```
Add physical frame allocator

Implements a bump-style allocator over conventional memory
as described in ADR-0003. Skips low memory for early safety.

Fixes #42
```

### Pull Request Workflow

1. **Before submitting:**
   - Run `cargo xtask doctor` to verify your setup
   - Build locally: `cargo xtask build-uefi && cargo xtask build-kernel`
   - Run tests: `cargo test --workspace`
   - Format: `cargo fmt --check`
   - Lint: `cargo clippy -- -D warnings`

2. **Submit a PR:**
   - Push your branch: `git push origin feature/my-feature`
   - Open a pull request against `main`
   - Fill in the PR template
   - Link any related issues

3. **After review:**
   - Address feedback
   - Squash commits if requested
   - Merge once CI passes

## Areas Where Help Is Needed

### Good First Issues

- Documentation improvements
- Test coverage for shared/abi crate
- Code cleanup and documentation comments

###medium-Effort Issues

- Kernel logging framework
- Error handling / panic strategy
- Virtual memory and page tables

### Advanced Issues

- User/kernel ABI boundaries
- ELF loader
- Syscall implementation

## Project Structure

```
docs/          - ADRs, phase docs, architecture notes
boot/         - UEFI loader entry point
kernel/       - no_std kernel core
shared/abi    - types shared between kernel and userland
tools/xtask   - build automation
```

See [docs/architecture.md](docs/architecture.md) for details.

## Communication

- Open an issue for bugs or feature requests
- Use GitHub Discussions for questions
- Be respectful and constructive

## License

By contributing, you agree that your contributions will be licensed under the project's license (MIT or Apache-2.0).