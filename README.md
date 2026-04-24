# NewOS

A Rust-first operating system built step by step for learning and long-term usability.

<p align="center">
  <img src="docs/images/qemu-boot-demo.gif" alt="NewOS boot demo" width="80%" />
</p>

## Current Status

| Phase | Status | Milestone |
|-------|--------|-----------|
| 0 | ✅ Complete | Foundation & scaffold |
| 1 | ✅ Complete | First boot & UEFI loader |
| 2 | ✅ Complete | Freestanding kernel handoff |
| 3 | ✅ Complete | Physical memory bring-up |
| 4 | 🔄 In Progress | Interrupts & timers |
| 5 | 🚧 Pending | Execution & syscalls |
| 6 | 🚧 Pending | Terminal-first usability |
| 7 | 🚧 Pending | Wayland desktop path |

## What Works

- Verified UEFI loader → freestanding kernel handoff in QEMU
- `x86_64-unknown-none` kernel image with serial output
- Physical frame allocator + bump heap with `alloc` crate support
- GDT, IDT, TSS, and PIC setup with hardware timer interrupts

## Quick Start

```powershell
#Install build tools (see docs/windows-host-setup.md)
cargo xtask doctor

#Build and run in QEMU
cargo xtask run-uefi
```

See [docs/quickstart.md](docs/quickstart.md) for full setup.

## Documentation

- [Architecture](docs/architecture.md)
- [Roadmap](docs/roadmap.md)
- [Phase Docs](docs/) - detailed phase breakdowns

## Repository Layout

```
docs/          - project documentation, ADRs, and guides
boot/         - UEFI firmware-facing entry points
kernel/       - freestanding kernel core
shared/abi    - shared types for kernel/userland boundary
tools/xtask   - developer automation
```

## Principles

- Learn deeply while building something real
- Prefer modern, maintainable designs over clever shortcuts
- Use open standards where compatibility matters
- Keep interfaces explicit so parts can be upgraded cleanly
- Write docs as we go so future changes stay understandable

## Technology Stack

- Language: Rust (nightly, `no_std`)
- Target: `x86_64-unknown-none`
- Build: custom `xtask` automation
- Testing: QEMU + OVMF

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). All contributions welcome!

## License

Licensed under MIT or Apache-2.0. See [LICENSE](LICENSE).

## Contact

- Open an issue for bugs or feature requests
- Discuss in GitHub Discussions