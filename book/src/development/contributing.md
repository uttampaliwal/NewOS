# Contributing

## Fork Workflow

1. Fork the repository on GitHub
2. Clone your fork: `git clone https://github.com/<your-username>/turnix.git`
3. Add upstream: `git remote add upstream https://github.com/uttampaliwal/turnix.git`
4. Create a feature branch: `git checkout -b feature/my-feature`

## Development Setup

Prerequisites: Rust nightly, QEMU with OVMF, and `cargo xtask` for build automation.

```bash
rustup install nightly && rustup default nightly
rustup target add x86_64-unknown-uefi x86_64-unknown-none
cargo xtask doctor   # verify environment
```

## Coding Standards

- Kernel code uses `no_std`; never import `std` in kernel crates
- Run `cargo fmt --check` and `cargo clippy -- -D warnings` before committing
- Avoid `.unwrap()` in kernel code — handle errors explicitly
- Use `#[must_use]` for functions returning important values
- Keep commit subject lines under 72 characters, use imperative mood

## Pull Request Process

1. Build locally: `cargo xtask build-uefi && cargo xtask build-kernel`
2. Run all tests: `cargo test --workspace`
3. Push your branch and open a PR against `development`
4. Link related issues and fill in the PR template
5. Address review feedback; squash commits if requested
