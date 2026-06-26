# Known Issues

> Known limitations that require future work. Each item includes the impact,
> root cause, and proposed fix. Fully resolved items have been removed —
> see git history for the complete list.

---

## 1. ext4 Writes Are In-Memory Only

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/fs/ext4/mod.rs`, `kernel/src/fs/ext4/state.rs` |
| **Status** | Resolved |

**Resolution:** Replaced tmpfs delegation with proper ext4 state management
(`Ext4State`). All metadata and file data is stored in-memory using ext4
structures (inodes, directory entries, extent stubs, xattrs). Dirty tracking
is implemented. `sync()` now writes dirty inodes and superblock to the block
device when an `Ext4Device` is attached. Block allocator with free-set
tracking implemented. `write_inode()` and `write_superblock()` persist
metadata to disk via the block device interface.

---

## 2. Kernel GP Fault During Userspace Scheduling (Mitigated)

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

## 3. ~~No Performance Tracing (ftrace, kprobes)~~ (Resolved)

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/tracing/` |
| **Status** | Resolved |

**Resolution:** Full in-kernel tracing subsystem implemented across four modules:

- **Base trace buffer** (`mod.rs`): bounded ring-buffered event store with category-based
  recording, global `trace()` API, `trace_event!` macro, snapshot/clear operations.
- **Function tracer** (`function_trace.rs`): per-function entry/exit tracing with TSC
  timestamps, per-CPU trace buffers (64 CPUs supported), function registry with
  name/module lookup, formatted output with `drain_to_global()` for integration.
- **Kprobes** (`kprobes.rs`): dynamic kernel probes at arbitrary instruction addresses,
  register/ unregister/arm/disarm lifecycle, register snapshot (RIP/RDI/RSI/RDX/RCX/R8/R9),
  address-based lookup, event buffer with eviction.
- **Trace pipe** (`trace_pipe.rs`): unified streaming output merging all sources (base buffer,
  function trace, kprobes) with category prefix filtering, `read_line()`/`peek_line()`/
  `read_all()` API, rewind/clear, global singleton with `refresh()`.

All 39 tracing tests pass. Total test count: 979.

**Tracking:** `docs/roadmap.md` Phase 14, `docs/sota-gap-analysis.md` #10

---

## 4. ~~No Memory Compression (zswap/zram)~~ (Resolved)

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/memory/` |
| **Status** | Resolved |

**Resolution:** Full memory compression subsystem implemented across three modules:

- **Compression engine** (`compress.rs`): tag-based encoding with literal runs and back-references
  (0x00 = literal, 0x01 = back-ref), `find_match()` with 4 KiB search window, up to 65540-byte
  matches. 15 compression tests.
- **zswap** (`zswap.rs`): compressed write-back cache in front of the swap device. Stores compressed
  pages in a bounded vector with LRU eviction. Falls through to backing device on cache miss.
  `MockSwapDevice` for testing. 14 tests including store/retrieve, LRU eviction, stats tracking.
- **zram** (`zram.rs`): compressed block device in RAM (IS the swap device, no backing store).
  Implements `SwapDevice` trait for drop-in use with the swap manager. Per-slot compression with
  stats tracking. 15 tests including round-trips, compression ratios, overwrite, and trait API.

Total: 1023 tests pass.

---

## 5. No Crash Dump / Reliability Engineering

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/panic.rs`, `kernel/src/reliability/` (new) |
| **Status** | Partially Addressed |

**Impact:** Kernel panics produce only a register dump. No crash dump is
captured for post-mortem analysis. No watchdog for hang detection. No fault
injection for robustness testing. Production kernels require all three.

**Progress:** Panic crash-dump capture, register dump formatting, and recent
kernel-log snapshotting are now implemented in `kernel/src/crash_dump.rs` and
wired into the panic path. Watchdog and fault-injection support remain open.

**Proposed Fix:**
- Crash dump: kdump-style capture of kernel memory to reserved region
- Watchdog: hardware watchdog timer (HPET/LAPIC) with pre-panic countdown
- Fault injection: configurable failure points for alloc, I/O, network
- Panic reports: structured JSON logs with backtrace, oops decoding
- Kernel checkpoints: save/restore state for live migration

**Tracking:** `docs/roadmap.md` Phase 15

---

## 6. No Kernel Crypto API

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/crypto.rs` |
| **Status** | Resolved |

**Resolution:** Full kernel crypto API implemented with:
- `Digest` trait with streaming SHA-256 (incremental update/finalize)
- `Mac` trait with HMAC-SHA256
- ChaCha20-based CSPRNG seeded from RDRAND + TSC jitter entropy
- HKDF (RFC 5869) key derivation (extract + expand)
- PBKDF2-HMAC-SHA256 password hashing (replaces insecure DJB2)
- Constant-time comparison for all MAC/password verification
- 14 tests covering all crypto primitives

---

## 7. No Device Driver PM / Hotplug Framework

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/drv/` |
| **Status** | Open |

**Impact:** Drivers are standalone with no power management or hotplug
support. Cannot suspend/resume, cannot handle USB/PCI hot-plug events,
cannot do runtime power management. Laptops and servers require all three.

**Proposed Fix:**
- Driver model: Bus, Device, Driver trait objects with probe/remove/suspend/resume
- Runtime PM: reference-counted autosuspend, runtime_get_sync/put_suspend
- System PM: suspend-to-idle, suspend-to-RAM, hibernation
- Hotplug: USB device insertion/removal events, PCI hot-add/hot-remove
- Device tree or ACPI-based enumeration

**Tracking:** `docs/roadmap.md` Phase 16

---

## 8. No Hypervisor / Virtualization Support

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/kvm/` (new) |
| **Status** | Open |

**Impact:** Cannot run guest VMs. Modern OSes increasingly support
virtualization for containers (KVM), security sandboxing, and running
legacy software.

**Proposed Fix:**
- VT-x/AMD-V: VMCS/VMCB management, VM entry/exit handling
- Nested paging: EPT/NPT for guest physical → host physical mapping
- Virtual devices: VirtIO net, block, console, input for guests
- /dev/kvm interface for userspace hypervisors
- VM launch: load ELF kernel into guest physical memory, set up CR3/CR4

**Tracking:** `docs/roadmap.md` Phase 16

---

## 9. No Userspace Coreutils / POSIX Utilities

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `userland/coreutils/` (new) |
| **Status** | Open |

**Impact:** No standard UNIX utilities (ls, cat, grep, cp, mv, rm, chmod,
etc.). Users must write custom programs for basic operations. An OS without
coreutils is not usable for development or daily use.

**Proposed Fix:** Port or rewrite core utilities:
- File operations: cat, cp, mv, rm, ln, mkdir, rmdir, chmod, chown, chgrp
- Text processing: grep, sed, awk, sort, uniq, wc, head, tail, cut, tr
- System info: ps, top, df, du, free, uname, uptime, whoami, id
- Process management: kill, nice, nohup, sleep, wait
- Archives: tar, gzip, gunzip
- Network: curl, wget, ssh, scp

**Tracking:** `docs/roadmap.md` Phase 15

---

## 10. ~~No io_uring or Zero-Copy Networking~~ (Resolved)

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/ipc/io_uring.rs` |
| **Status** | Resolved |

**Resolution:** io_uring implemented with 12 operations (NOP, Read, Write,
Close, Openat, Fsync, Statx, Send, Recv, PollAdd, PollRemove, Timeout),
9 tests, eventfd notification integration. Syscalls 85-87 registered.
Zero-copy networking remains future work.

---

## 11. ~~No Huge Pages, THP, NUMA, or KSM~~ (Partially Addressed)

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/memory/` |
| **Status** | Partially Addressed |

**Impact:** 4KB pages only. TLB pressure is high on large-memory workloads.
No NUMA awareness means poor performance on multi-socket systems.

**Progress:** All four core subsystems implemented with full test coverage:
- **Huge Pages** (`hugepage.rs`): 2MB/1GB pool allocator with alloc/free/stats, 12 tests
- **KSM** (`ksm.rs`): content-hash-based page deduplication with stable tree, COW fault handling, 12 tests
- **NUMA** (`numa.rs`): multi-node tracking, 5 allocation policies (Local/Bind/Interleave/Preferred/Default), distance matrix, memory tiers, 18 tests
- **THP** (`thp.rs`): region-based promotion/demotion with access-count threshold, 15 tests

**Remaining:**
- Memory compression (zswap/zram) — tracked as separate item
- Memory compaction for contiguous allocations
- Integration with page fault handler for automatic THP promotion
- hugetlbfs mount point and sysctl interface
- NUMA page migration and memory hotplug

**Tracking:** `docs/roadmap.md` Phase 16

---

## 12. No Workqueues, Softirqs, or Tasklets

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/sync/workqueue.rs`, `kernel/src/softirq.rs` |
| **Status** | Resolved |

**Resolution:** Workqueue and softirq implemented in Phase 12. WorkQueue provides
FIFO function-pointer dispatch. Softirq provides 8 named vectors with bitmask
tracking. Tasklets are now implemented on top of the softirq tasklet vector as
a small deferred callback queue.

---

## 13. No KASAN/KFENCE Memory Safety Detection

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/memory/kasan.rs` |
| **Status** | Resolved |

**Resolution:** KASAN implemented with shadow memory (1 byte per 8 bytes
heap), poisoning (0x6b alloc, 0xbb redzone, 0xbb freed), range validation
(check_range), violation reporting with shadow dump, kernel dmesg stats
command, validate_user_ptr/validate_kernel_buf APIs. Integrated into
fixed_size_block allocator. KFENCE wired into the fixed-size heap allocator
as a sampling path. Runtime detection now active:
- KASAN: double-free detection in free_poison, check_range_access for
  user-space pointer validation in copy_from_user/copy_to_user
- KFENCE: canary scanning on free catches heap buffer overflows before
  UAF overwrite, stats exposed via kfence_stats_string()

---

## 14. No Lockdep or Completion Variables

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/sync/lockdep.rs` (new) |
| **Status** | Resolved |

**Resolution:** SeqLock and RwLock implemented in Phase 12. SeqLock provides
optimistic reader / exclusive writer. RwLock provides multiple-reader /
single-writer. Lockdep now tracks lock-order inversions and completion
variables provide one-shot wait/signal synchronization.

---

## 15. No Container Runtime or OCI Support

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `userland/containerd/` (new) |
| **Status** | Open |

**Impact:** Cannot run OCI containers. Namespaces and cgroups v2 exist
but there is no container runtime to manage image layers, networking,
and lifecycle.

**Proposed Fix:**
- OCI runtime: container creation, lifecycle, spec parsing
- OverlayFS: union mount for image layers
- Device cgroups: control device access per container
- Checkpoint/restore: CRIU integration for live migration
- Container networking: veth pairs, bridge, network namespaces

**Tracking:** `docs/roadmap.md` Phase 18

---

## 16. Undocumented Unsafe Blocks (~153 remaining)

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/` (all subsystems) |
| **Status** | Resolved |

**Resolution:** All `unsafe { ... }` blocks (statements, not declarations)
now have `// Safety:` comments. 59 blocks documented across 12 files in this
session (syscall handlers, memory allocators, boot, signals, ext2). Combined
with prior work, 404 total `unsafe` blocks in `kernel/src/` are fully
documented. The lint is currently `#![allow(clippy::undocumented_unsafe_blocks)]`
in `kernel/src/lib.rs` — can be switched to `#![warn(...)]` now that
backfill is complete.

---

## 17. Uneven Test Coverage

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/task/`, `kernel/src/net/` |
| **Status** | Improved |

**Resolution:** Overall test count grew to 882 tests across the kernel library
suite. Signal delivery, scheduler behavior, cgroup boundary handling, and
network socket state handling are now covered by targeted regression tests.
The remaining weak areas are scheduler SMP load balancing and broader network
state-machine coverage.

**Remaining:** Scheduler SMP load balancing and cgroup enforcement tests
are still missing. Networking (`net/socket.rs`) has 8 tests, all
SocketTable bookkeeping — zero syscall or state machine tests
(bind/listen/connect/accept/recv/send). `net/smoltcp_iface.rs` has 7
tests for basic lifecycle only — no data path or connection tests.

**Proposed Fix:**
- Scheduler: tests for SMP load balancing, CFS vruntime fairness,
  scheduler class switching, cgroup enforcement under contention
- Networking: socket state machine tests (LISTEN→ESTABLISHED→CLOSE),
  TCP retransmission, concurrent accept()

**Tracking:** `docs/sota-gap-analysis.md` #1 (Scalability)

---

## Summary

| # | Issue | Severity | Status |
|---|-------|----------|--------|
| 1 | ext4 writes are in-memory only | Medium | Resolved |
| 2 | GP fault during fork/clone | Medium | Mitigated |
| 3 | No performance tracing (ftrace, kprobes) | High | Open |
| 4 | No memory compression (zswap/zram) | Medium | Resolved |
| 5 | No crash dump / reliability engineering | High | Partially Addressed |
| 6 | No kernel crypto API | Medium | Resolved |
| 7 | No device driver PM / hotplug framework | Medium | Open |
| 8 | No hypervisor / virtualization support | Medium | Open |
| 9 | No userspace coreutils / POSIX utilities | Medium | Open |
| 10 | No io_uring or zero-copy networking | High | Resolved |
| 11 | No huge pages, THP, NUMA, or KSM | High | Partially Addressed |
| 12 | No workqueues, softirqs, or tasklets | High | Resolved |
| 13 | No KASAN/KFENCE memory safety detection | High | Resolved |
| 14 | No lockdep or completion variables | Medium | Resolved |
| 15 | No container runtime or OCI support | Medium | Open |
| 16 | Undocumented unsafe blocks | Medium | Resolved |
| 17 | Uneven test coverage | Medium | Improved |
