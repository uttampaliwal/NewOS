# NewOS

<p align="center">
  <a href="https://github.com/uttampaliwal/NewOS/actions/workflows/ci.yml">
    <img src="https://img.shields.io/github/actions/status/workflow/uttampaliwal/NewOS/ci?style=flat-square" alt="CI Status" />
  </a>
  <a href="https://crates.io/crates/newos-kernel">
    <img src="https://img.shields.io/badge/rustc-nightly-blue?style=flat-square" alt="Rust Version" />
  </a>
  <a href="LICENSE">
    <img src="https://img.shields.io/badge/license-MIT%20or%20Apache--2.0-blue?style=flat-square" alt="License" />
  </a>
  <a href="https://github.com/uttampaliwal/NewOS/issues">
    <img src="https://img.shields.io/github/issues-raw/uttampaliwal/NewOS?style=flat-square" alt="Issues" />
  </a>
</p>

**A Rust-first operating system built step by step for learning and long-term usability.**

> *Exploring the potential of AI tools in building a capable and usable operating system.*

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
- Cooperative kernel multitasking

## Quick Start

```powershell
# Install build tools (see docs/windows-host-setup.md)
cargo xtask doctor

# Build and run in QEMU
cargo xtask run-uefi
```

See [docs/quickstart.md](docs/quickstart.md) for full setup.

## Mission

Build an understandable, replaceable, and well-documented OS from first principles — while learning deeply and using modern tools.

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

## Contributing

We welcome contributions! Start by:

1. Picking a [good first issue](https://github.com/uttampaliwal/NewOS/labels/good%20first%20issue)
2. Reading [CONTRIBUTING.md](CONTRIBUTING.md)
3. Joining the discussion in [GitHub Discussions](https://github.com/uttampaliwal/NewOS/discussions)

## Technology Stack

- Language: Rust (nightly, `no_std`)
- Target: `x86_64-unknown-none`
- Build: custom `xtask` automation
- Testing: QEMU + OVMF

## License

Licensed under MIT or Apache-2.0. See [LICENSE](LICENSE).

## Contact

- Open an [issue](https://github.com/uttampaliwal/NewOS/issues) for bugs or feature requests
- Discuss in [GitHub Discussions](https://github.com/uttampaliwal/NewOS/discussions)