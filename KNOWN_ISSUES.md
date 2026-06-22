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

## 2. EVM HMAC Key Is Hardcoded

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/security/ima.rs` |
| **Impact** | EVM integrity verification is compromised if source is leaked. |

**Root Cause:** The EVM HMAC key is a compile-time constant:
```rust
const EVM_HMAC_KEY: &[u8; 32] = b"turnix-evm-hmac-key-2024-v1!1234";
```
The TPM driver exists (`kernel/src/drivers/tpm.rs`) but is not wired into
key derivation. The TPM probe rejects invalid devices (VID=0) but the
initialization sequence does not derive keys from the TPM's sealed storage.

**Proposed Fix:**
1. At boot, derive the EVM key from TPM-stored seed via `TPM2_CC_UNSEAL`
2. Store derived key in a kernel-protected memory region
3. Remove the hardcoded constant
4. Add key rotation support

**Tracking:** `.kiro/specs/turnix-production-readiness/gap-fixes.md` GCUT-1

---

## 3. Kernel GP Fault During Userspace Scheduling

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/process.rs`, `kernel/src/syscall/handler.rs` |
| **Impact** | Fork/clone syscalls may trigger a GP fault in QEMU. |

**Root Cause:** A pre-existing GP fault occurs during fork/clone execution.
The fault is unrelated to recent Phase 3 changes — it reproduces on the
codebase before any of our modifications. The fault prevents full validation
of userspace process scheduling, including exec, wait, and signal delivery.

**Proposed Fix:** Debug the GP fault in the fork/clone path:
1. Audit the context-switch frame layout for correctness
2. Verify CS/SS/RFLAGS values saved during fork
3. Check that the new process page tables are correctly set up
4. Test with QEMU's `-d int` flag to identify the faulting instruction

**Tracking:** This issue predates the production-readiness spec.

---

## 4. Worker UART Busy-Wait Starves Serial Writes

| | |
|---|---|
| **Severity** | Low |
| **Component** | `kernel/src/boot.rs` (line ~482) |
| **Impact** | Userspace serial output is delayed during driver initialization. |

**Root Cause:** The worker task that initializes hardware drivers uses a
busy-wait loop for UART output. While this loop is running, the scheduler
cannot preempt it, so userspace processes that write to serial (e.g., the
init process) are blocked until the driver init completes.

**Proposed Fix:** Convert the worker task to use interrupt-driven serial
output or add yield points in the initialization loop.

**Tracking:** This issue predates the production-readiness spec.

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
| 2 | EVM HMAC key is hardcoded | High | Open |
| 3 | GP fault during fork/clone | Medium | Open |
| 4 | UART busy-wait starves serial | Low | Open |
| 5 | Branch naming inconsistency | Low | Resolved |
| 6 | Rust toolchain not pinned | Low | Resolved |
