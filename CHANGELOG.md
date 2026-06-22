# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Known limitations document (KNOWN_ISSUES.md)
- TPM 2.0 TIS driver with probe/initialize/seal/unseal
- ext2 block allocator, inode allocator, directory entry support
- Network interface syscalls (NetSetAddr, NetSetRoute, NetQuery)
- Package manager network fetcher (PackageFetcher, HTTP client, SHA-256 verification)
- XHCI extended capability parsing and USB legacy support handoff
- Kernel time module (uptime_us via scheduler ticks)

### Changed
- Updated all documentation to reflect current project state
- Removed redundant docs (Improvements.md, architecture-diagram.md, gap-analysis-and-roadmap.md, roadmap-timeline.md, DOCS_BUILD_ON_WINDOWS.md)
- TPM probe rejects invalid devices (VID=0)
- LSM hook initialization message now includes MAC hooks

### Fixed
- Packed struct field access in ext2 write tests
- Package-manager compilation errors (verify_sha256, PackageFetcher, RepositoryClient)
- Clippy warnings (div_ceil, repeat_n, unused vars, Safety docs)

## [v0.0.7] - 2026-06-21

### Added
- Phase 7: Desktop Environment
  - Window Compositor (Wayland-like)
  - Input event routing
  - Desktop session management
  - EDID display mode parser
  - Double-buffering and VBlank support
  - Dynamic desktop resolution

## [v0.0.6] - 2026-06-20

### Added
- Phase 6: System Services Layer
  - IPC Broker daemon with credential-based authentication
  - Structured Logger with HMAC-SHA256 seals
  - Service Manager with restart policies and process supervision
  - Network Manager with DHCP client and static IP configuration
  - Device Manager with hotplug event handling
  - Kernel Network Stack (smoltcp TCP/IP)

## [v0.0.5] - 2026-06-19

### Added
- Phase 5: Package Management
  - tpkg-format crate (manifest serialization/deserialization)
  - CDCL SAT solver for dependency resolution
  - TUF repository client with signature verification
  - Rollback pipeline with install staging and snapshots
- Phase 4: Security Hardening (complete)
  - POSIX Capabilities (64-bit capability sets)
  - Namespaces (Mount, Network, PID, User)
  - Seccomp-BPF filters
  - LSM Hook Framework with DAC implementation
  - IMA/EVM integrity measurement
  - Stack Canaries
- Phase 3: POSIX-Compatible System Services (complete)
  - Process table with fork/exec/waitpid
  - VFS with tmpfs and ext4 backends
  - Unix domain sockets and pipes
  - Console redirection (stdin/stdout/stderr)
  - Init daemon with service manifests

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

[unreleased]: https://github.com/uttampaliwal/turnix/compare/v0.0.7...HEAD
[v0.0.7]: https://github.com/uttampaliwal/turnix/compare/v0.0.6...v0.0.7
[v0.0.6]: https://github.com/uttampaliwal/turnix/compare/v0.0.5...v0.0.6
[v0.0.5]: https://github.com/uttampaliwal/turnix/compare/v0.0.4...v0.0.5
[v0.0.4]: https://github.com/uttampaliwal/turnix/compare/v0.0.3...v0.0.4
[v0.0.3]: https://github.com/uttampaliwal/turnix/compare/v0.0.2...v0.0.3
[v0.0.2]: https://github.com/uttampaliwal/turnix/compare/v0.0.1...v0.0.2
[v0.0.1]: https://github.com/uttampaliwal/turnix/commits/v0.0.1
