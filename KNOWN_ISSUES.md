# Known Issues

> Known limitations that require future work. Each item includes the impact,
> root cause, and proposed fix.

---

## 1. ext4 Writes Delegate to Tmpfs

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/fs/ext4/mod.rs` |
| **Impact** | No persistent storage. All writes are lost on reboot. |

**Root Cause:** The `Ext4Backend` delegates all write operations (`write`, `mkdir`,
`unlink`, `rename`) to an in-memory `TmpfsBackend`. The on-disk ext4 structures
(superblock, block groups, inode table) are parsed read-only. There is no block
allocator, no journal, and no write-back path.

**Proposed Fix:** Implement the full ext4 write path:
1. Block allocator (bitmap manipulation)
2. Inode allocator
3. Data block allocation with extent tree
4. Directory modification (add/remove entries)
5. Journal (ext3-style) for crash consistency
6. Write-back dirty page tracking and flush to NVMe

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

## Summary

| # | Issue | Severity | Status |
|---|-------|----------|--------|
| 1 | ext4 writes delegate to tmpfs | High | Open |
| 2 | EVM HMAC key is hardcoded | High | Resolved |
| 3 | GP fault during fork/clone | Medium | Mitigated |
| 4 | UART busy-wait starves serial | Low | Resolved |
| 5 | Branch naming inconsistency | Low | Resolved |
| 6 | Rust toolchain not pinned | Low | Resolved |
