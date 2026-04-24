# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Documentation: architecture diagrams, roadmap timeline
- CI: QEMU smoke test workflow
- Developer guides: good first issues, contributing workflow

## [v0.0.4] - 2026-04-24

### Added
- Cooperative kernel multitasking (Phase 5)
- GDT, IDT, TSS, and hardware timer interrupts (Phase 4)
- Virtual memory and heap bootstrap

### Fixed
- Whitespace in authors list in Cargo.toml

## [v0.0.3] - 2026-04-23

### Added
- Physical frame allocator bring-up
- Freestanding kernel handoff
- Explicit kernel handoff contract

## [v0.0.2] - 2026-04-22

### Added
- Phase 1 UEFI first-boot path

## [v0.0.1] - 2026-04-21

### Added
- Initial repository scaffold (Phase 0)
- Workspace structure with boot/, kernel/, shared/abi/, tools/xtask/
- Shared ABI crate with host-testable types
- xtask developer automation

[unreleased]: https://github.com/uttampaliwal/NewOS/compare/v0.0.4...HEAD
[v0.0.4]: https://github.com/uttampaliwal/NewOS/compare/v0.0.3...v0.0.4
[v0.0.3]: https://github.com/uttampaliwal/NewOS/compare/v0.0.2...v0.0.3
[v0.0.2]: https://github.com/uttampaliwal/NewOS/compare/v0.0.1...v0.0.2
[v0.0.1]: https://github.com/uttampaliwal/NewOS/commits/v0.0.1