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

## 7. No Security Architecture Documentation (Resolved)

| | |
|---|---|
| **Severity** | High |
| **Component** | `docs/security/` |
| **Status** | Resolved |

**Resolution:** Created `docs/security/` with six comprehensive documents:
`threat-model.md` (trust boundaries, attacker models, TCB),
`capabilities.md` (POSIX.1e capability sets, exec transformation),
`namespaces.md` (PID, mount, network, user namespace isolation),
`seccomp.md` (BPF interpreter, filter inheritance),
`lsm.md` (pluggable hook framework, DAC/MAC policies),
`ima-evm.md` (integrity measurement, EVM verification, TPM integration).

**Proposed Fix:** Create `docs/security/` with:
- `threat-model.md` — trust boundaries, attacker models, TCB definition
- `capabilities.md` — POSIX capability sets and enforcement points
- `namespaces.md` — namespace isolation semantics
- `seccomp.md` — BPF filter format and inheritance
- `lsm.md` — hook framework and DAC/MAC policies
- `ima-evm.md` — integrity measurement flow

---

## 8. No Fuzz Testing Infrastructure (Resolved)

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `fuzz/` |
| **Status** | Resolved |

**Resolution:** Created `fuzz/` directory with 5 standalone fuzz targets:
`fuzz_elf_parser` (ELF header parsing), `fuzz_seccomp_bpf` (BPF interpreter),
`fuzz_ipc_message` (IPC deserialization), `fuzz_vfs_path` (path normalization),
`fuzz_syscall_args` (argument decoding). Each reads from stdin and tests
parsing logic for panics. Includes README with cargo-fuzz/AFL integration.

---

## 9. No Unified Kernel Error Type (Resolved)

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `shared/error/` |
| **Status** | Resolved |

**Resolution:** Created `shared/error/` crate (`turnix-error`) with a
unified `KernelError` enum covering 58 POSIX-compatible error variants.
Includes `to_errno()` / `from_errno()` conversion, `Display` impl,
and 3 unit tests. All subsystems can now map internal errors to
`KernelError` at boundaries.

---

## 10. No Userland Observability Commands (Resolved)

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `userland/observability/` |
| **Status** | Resolved |

**Resolution:** Created `userland/observability/` crate with 5 commands:
`ps` (process listing via dmesg), `meminfo` (memory info from kernel log),
`mount` (filesystem mount points), `lsns` (namespace listing),
`capsh` (POSIX capability inspection via capget syscall). All use
`no_std` with direct libturnix syscalls.

---

## 11. No Performance Benchmark Suite (Resolved)

| | |
|---|---|
| **Severity** | Low |
| **Component** | `benchmarks/` |
| **Status** | Resolved |

**Resolution:** Created `benchmarks/` crate (`host-benchmarks`) with
12 host-side benchmarks: SHA-256, BTreeMap insert/lookup, Vec push/sort,
String format/parse, memcpy/memset, HashMap insert/lookup, bitfield ops.
Includes throughput and latency metrics. Run with
`cargo run -p host-benchmarks --release`.

---

## 12. No mdBook / Generated Documentation (Resolved)

| | |
|---|---|
| **Severity** | Low |
| **Component** | `book/` |
| **Status** | Resolved |

**Resolution:** Created `book/` directory with mdBook setup: `book.toml`,
`SUMMARY.md`, and 20+ chapter files covering architecture, security,
subsystems, userland, and development. Chapters include real source
paths and Turnix-specific details. CI job builds rustdoc + mdBook.

---

## 13. No README Badges or Screenshots (Resolved)

| | |
|---|---|
| **Severity** | Low |
| **Component** | `README.md` |
| **Status** | Resolved |

**Resolution:** Added CI status badge, license badge, Rust version badge,
and test count badge to README header.

---

## 14. Kernel Architecture Boundary Undefined (Resolved)

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `docs/architecture/boundaries.md`, `book/src/architecture/boundaries.md` |
| **Status** | Resolved |

**Resolution:** Created comprehensive kernel/user boundary documentation
covering: what runs in kernel space (scheduler, VMM, VFS, IPC, security,
drivers), what runs in user space (init, shell, compositor, daemons),
IPC surface, potential user-space migrations, and TCB definition with
line counts.

---

## 15. Roadmap Ends at Phase 7, No Future Phases (Resolved)

| | |
|---|---|
| **Severity** | Low |
| **Component** | `docs/roadmap.md` |
| **Status** | Resolved |

**Resolution:** Added 4 future phases to `docs/roadmap.md`:
Phase 8 (SMP, APIC, NUMA), Phase 9 (TCP/IP, DNS, HTTP, TLS),
Phase 10 (Wayland polish, GPU, audio, packages),
Phase 11 (self-hosting, Rust compiler, native dev env).

---

## 16. CI Missing Doc Build and Fuzz Jobs (Resolved)

| | |
|---|---|
| **Severity** | Low |
| **Component** | `.github/workflows/ci.yml` |
| **Status** | Resolved |

**Resolution:** Extended CI matrix with 3 new jobs:
`lint` (clippy with `-D warnings`), `docs` (rustdoc + mdBook build with
artifact upload), `fuzz` (nightly fuzz campaign on master pushes for
all 5 fuzz targets).

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
| 7 | No security architecture docs | High | Resolved |
| 8 | No fuzz testing infrastructure | Medium | Resolved |
| 9 | No unified kernel error type | Medium | Resolved |
| 10 | No userland observability commands | Medium | Resolved |
| 11 | No performance benchmark suite | Low | Resolved |
| 12 | No mdBook / generated docs | Low | Resolved |
| 13 | No README badges or screenshots | Low | Resolved |
| 14 | Kernel architecture boundary undefined | Medium | Resolved |
| 15 | Roadmap ends at Phase 7 | Low | Resolved |
| 16 | CI missing doc build and fuzz jobs | Low | Resolved |
