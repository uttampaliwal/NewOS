Building notes

On Windows hosts, the kernel contains platform-specific assembly and linker assumptions that don't build reliably.

Recommended workflows:

- Local development (fast): run `cargo build` (no `--all`) to build default workspace members (userland libraries and tests). The kernel is excluded from default-members on Windows hosts.

- Full OS build (Linux/QEMU/CI): build on a Linux host or in Docker using the `xtask` helper or this Docker example:

  docker run --rm -v "$(pwd)":/work -w /work rust:latest bash -c "rustup default nightly && cargo build --all --target x86_64-unknown-none" 

- CI: ensure Linux runners build the kernel to validate all features and platform-specific code.

If you want me to add a Dockerfile/CI job to verify Linux builds, tell me and I'll add it.
