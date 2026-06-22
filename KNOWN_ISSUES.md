# Known Issues

> Known limitations that require future work. Each item includes the impact,
> root cause, and proposed fix.

---

## 1. ext4 Writes Are In-Memory Only

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/fs/ext4/mod.rs`, `kernel/src/fs/ext4/state.rs` |
| **Status** | Partially Resolved |

**Resolution:** Replaced tmpfs delegation with proper ext4 state management
(`Ext4State`). All metadata and file data is stored in-memory using ext4
structures (inodes, directory entries, extent stubs, xattrs). Dirty tracking
is implemented. All `FsBackend` operations work correctly (read, write,
mkdir, unlink, rename, readdir, stat, xattr).

**Remaining:** No block allocator, no journal, no write-back to disk via
block device. `sync()` marks all inodes clean but does not flush to NVMe.
This is expected for a memory-backed filesystem and does not affect
correctness for the current use case.

**Tracking:** `.kiro/specs/turnix-production-readiness/gap-fixes.md` GFS-2, GFS-4

---

## 2. EVM HMAC Key Is Hardcoded (Resolved)

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/security/ima.rs` |
| **Status** | Resolved |

**Resolution:** EVM HMAC key is now derived from TPM via `TPM2_CC_GET_RANDOM`
at first use. Falls back to hardcoded key when TPM is not available. Key is
stored in `Mutex<Option<[u8; 32]>>` and lazily initialized.

---

## 3. Kernel GP Fault During Userspace Scheduling (Mitigated)

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/process.rs`, `kernel/src/syscall/handler.rs` |
| **Status** | Mitigated |

**Mitigation:** Added RFLAGS sanitization in fork to clear dangerous bits
(IOPL, NT, VM). Added CS/SS validation to ensure user-mode selectors.
Improved GP fault handler with detailed register dump for debugging.
The root cause may still require QEMU-level debugging to fully resolve.

---

## 4. Worker UART Busy-Wait Starves Serial Writes (Resolved)

| | |
|---|---|
| **Severity** | Low |
| **Component** | `kernel/src/boot.rs` (line ~482) |
| **Status** | Resolved |

**Resolution:** Added bounded retry count (UART_TIMEOUT) to SerialWriter::write_byte()
to prevent infinite spinning. Added spin::Mutex for concurrent UART access safety.
Added yield_task() to worker_task() to prevent starvation.

---

## 5. Branch Naming Inconsistency (Resolved)

| | |
|---|---|
| **Severity** | Low |
| **Component** | Repository structure |
| **Status** | Resolved |

**Resolution:** The repository uses `master` as the production branch and
`development` as the integration branch (Gitflow model). All documentation
has been updated to reference `master` instead of `main`. CI triggers on
`master` for production builds.

---

## 6. Rust Toolchain Not Pinned (Resolved)

| | |
|---|---|
| **Severity** | Low |
| **Component** | `rust-toolchain.toml` |
| **Status** | Resolved |

**Resolution:** Toolchain pinned to `nightly-2026-06-22` in `rust-toolchain.toml`.

---

## 7. No Security Architecture Documentation

| | |
|---|---|
| **Severity** | High |
| **Component** | `docs/security/` |
| **Status** | Open |

**Impact:** Security is a core differentiator (capabilities, namespaces,
seccomp-BPF, LSM, IMA/EVM, ASLR/KASLR), but there is no threat model,
no per-subsystem security documentation, and no guidance for contributors
on security boundaries. The root `SECURITY.md` covers vulnerability
reporting but not architectural security posture.

**Proposed Fix:** Create `docs/security/` with:
- `threat-model.md` — trust boundaries, attacker models, TCB definition
- `capabilities.md` — POSIX capability sets and enforcement points
- `namespaces.md` — namespace isolation semantics
- `seccomp.md` — BPF filter format and inheritance
- `lsm.md` — hook framework and DAC/MAC policies
- `ima-evm.md` — integrity measurement flow

---

## 8. No Fuzz Testing Infrastructure

| | |
|---|---|
| **Severity** | Medium |
| **Component** | Repository-wide |
| **Status** | Open |

**Impact:** Kernel code (ELF parser, syscall decoder, VFS path resolver,
seccomp BPF interpreter, IPC messages) has no fuzz coverage. These are
high-value targets for memory corruption and logic bugs.

**Proposed Fix:** Add `fuzz/` directory with `cargo-fuzz` targets:
- ELF parser fuzzer
- Syscall decoder fuzzer
- VFS path parser fuzzer
- Seccomp BPF filter fuzzer
- IPC message fuzzer

Integrate into CI as a nightly fuzzing job.

---

## 9. No Unified Kernel Error Type

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `shared/` |
| **Status** | Open |

**Impact:** Each subsystem defines its own error types ad-hoc. As the kernel
grows, inconsistent error handling across VFS, syscalls, drivers, and IPC
increases maintenance burden and makes error propagation harder to reason
about.

**Proposed Fix:** Create `shared/error/` crate with a unified `KernelError`
enum:
```rust
enum KernelError {
    InvalidAddress,
    PermissionDenied,
    OutOfMemory,
    InvalidFileDescriptor,
    DeviceNotReady,
    IoError,
    NotFound,
    AlreadyExists,
    // ...
}
```
Each subsystem maps its internal errors to `KernelError` at boundaries.

---

## 10. No Userland Observability Commands

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `userland/` |
| **Status** | Open |

**Impact:** Only `dmesg` exists as a syscall shim. No `ps`, `meminfo`,
`mount`, `lsns`, `capsh`, or `top` commands. Debugging kernel state
requires serial output, which slows development and makes the system
feel incomplete for end users.

**Proposed Fix:** Add userland binaries:
- `ps` — process table dump (PID, state, PPID, command)
- `meminfo` — memory statistics (used, free, cached, swap)
- `mount` — VFS mount points and filesystem types
- `lsns` — active namespaces
- `capsh` — capability state inspection

---

## 11. No Performance Benchmark Suite

| | |
|---|---|
| **Severity** | Low |
| **Component** | Repository-wide |
| **Status** | Open |

**Impact:** CI references `cargo xtask ci-bench` but there is no standalone
`benchmarks/` directory, no benchmark harness, and no historical performance
tracking. Boot time, context switch latency, IPC throughput, and filesystem
throughput are unknown.

**Proposed Fix:** Create `benchmarks/` directory with criterion-based
micro-benchmarks for:
- Boot-to-shell time
- Context switch latency
- Pipe/Unix socket throughput
- VFS operation latency
- Memory allocator throughput

Add historical tracking via CI artifacts or a results dashboard.

---

## 12. No mdBook / Generated Documentation

| | |
|---|---|
| **Severity** | Low |
| **Component** | `docs/` |
| **Status** | Open |

**Impact:** Documentation exists as individual markdown files in `docs/`
and ADRs, but there is no generated book, no rustdoc deployment, and no
searchable browsable documentation site. New contributors must manually
navigate files.

**Proposed Fix:**
- Add `book.toml` and `book/` directory for mdBook
- Structure: introduction, kernel architecture, drivers, syscalls, security
- Deploy to GitHub Pages via CI
- Add `cargo doc --workspace` to CI and deploy rustdoc

---

## 13. No README Badges or Screenshots

| | |
|---|---|
| **Severity** | Low |
| **Component** | `README.md` |
| **Status** | Open |

**Impact:** No CI status badges, no license badge, no test count badge,
no QEMU boot screenshots or GIFs. Repository discoverability and
first impressions suffer.

**Proposed Fix:** Add to README header:
```md
![CI](https://github.com/uttampaliwal/turnix/actions/workflows/ci.yml/badge.svg)
![License](https://img.shields.io/badge/license-MIT-blue)
![Rust](https://img.shields.io/badge/rust-nightly-orange)
```
Add QEMU boot screenshot and shell session GIF.

---

## 14. Kernel Architecture Boundary Undefined

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `README.md`, `docs/architecture.md` |
| **Status** | Open |

**Impact:** README describes Turnix as a "microkernel/modular monolith
hybrid" but does not define: what runs in kernel space vs user space,
which services could be moved out, what the IPC boundaries are, or what
the TCB (Trusted Computing Base) includes. OS developers evaluating the
project immediately ask these questions.

**Proposed Fix:** Add explicit kernel/user boundary documentation:
- Kernel space: scheduler, VMM, IPC, VFS, security hooks, drivers
- User space: init, shell, compositor, network services
- Document which drivers are in-kernel vs could be moved to user space
- Define IPC surface between kernel and userland services

---

## 15. Roadmap Ends at Phase 7, No Future Phases

| | |
|---|---|
| **Severity** | Low |
| **Component** | `docs/roadmap.md` |
| **Status** | Open |

**Impact:** Roadmap shows all 7 phases as "Complete" with no forward-looking
phases. Contributors have no visibility into planned work (SMP, networking
maturity, self-hosting, desktop polish).

**Proposed Fix:** Add future phases to `docs/roadmap.md`:
- **Phase 8**: SMP, APIC, NUMA awareness
- **Phase 9**: TCP/IP stack maturity, DNS, HTTP client
- **Phase 10**: Wayland compositor polish, GPU acceleration, package repo
- **Phase 11**: Self-hosting toolchain, Rust compiler port

---

## 16. CI Missing Doc Build and Fuzz Jobs

| | |
|---|---|
| **Severity** | Low |
| **Component** | `.github/workflows/ci.yml` |
| **Status** | Open |

**Impact:** CI has `build`, `unit-tests`, `boot-gate`, `driver-tests`,
`security-regression`, and `performance-benchmarks`. Missing:
- `cargo doc` build and deploy (GitHub Pages)
- Nightly fuzzing job (cargo-fuzz / libfuzzer)
- Clippy lint job (currently manual)

**Proposed Fix:** Extend CI matrix with:
- `docs` job: build and deploy mdBook + rustdoc
- `fuzz` job: nightly cargo-fuzz runs with regression detection
- `lint` job: clippy with `-D warnings`

---

## Summary

| # | Issue | Severity | Status |
|---|-------|----------|--------|
| 1 | ext4 writes are in-memory only | Medium | Partially Resolved |
| 2 | EVM HMAC key is hardcoded | High | Resolved |
| 3 | GP fault during fork/clone | Medium | Mitigated |
| 4 | UART busy-wait starves serial | Low | Resolved |
| 5 | Branch naming inconsistency | Low | Resolved |
| 6 | Rust toolchain not pinned | Low | Resolved |
| 7 | No security architecture docs | High | Open |
| 8 | No fuzz testing infrastructure | Medium | Open |
| 9 | No unified kernel error type | Medium | Open |
| 10 | No userland observability commands | Medium | Open |
| 11 | No performance benchmark suite | Low | Open |
| 12 | No mdBook / generated docs | Low | Open |
| 13 | No README badges or screenshots | Low | Open |
| 14 | Kernel architecture boundary undefined | Medium | Open |
| 15 | Roadmap ends at Phase 7 | Low | Open |
| 16 | CI missing doc build and fuzz jobs | Low | Open |
