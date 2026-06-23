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

## 3. No Performance Tracing (ftrace, kprobes)

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/tracing/` (new) |
| **Status** | Open |

**Impact:** No visibility into kernel internals. Cannot diagnose latency,
contention, or performance regressions. All production kernels require
observability infrastructure.

**Proposed Fix:** Implement:
- ftrace framework: function tracer, function_graph, trace events via tracefs
- kprobes: dynamic instrumentation at any kernel function
- uprobes: dynamic instrumentation at user-space addresses
- perf: hardware performance counter abstraction (PMU)
- trace output to ring buffer, readable via /sys/kernel/debug/tracing

**Tracking:** `docs/roadmap.md` Phase 14, `docs/sota-gap-analysis.md` #10

---

## 4. No Memory Compression (zswap/zram)

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/mm/swap.rs` |
| **Status** | Open |

**Impact:** Swap goes directly to block device. Compressed swap (zswap/zram)
can hold 2-3x more data in RAM, reducing disk I/O and improving
responsiveness under memory pressure.

**Proposed Fix:**
- zswap: compressed write-back cache in front of swap device
- zram: compressed block device in RAM
- LZ4 or ZSTD compression for swap pages
- same-page merging (KSM) for deduplication

---

## 5. No Crash Dump / Reliability Engineering

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/panic.rs`, `kernel/src/reliability/` (new) |
| **Status** | Open |

**Impact:** Kernel panics produce only a register dump. No crash dump is
captured for post-mortem analysis. No watchdog for hang detection. No fault
injection for robustness testing. Production kernels require all three.

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
| **Component** | `kernel/src/crypto/` (new) |
| **Status** | Open |

**Impact:** Cryptographic operations are scattered across IMA/EVM and
package verification. No unified in-kernel crypto API for encrypting
filesystems, network traffic, or key management.

**Proposed Fix:**
- AEAD ciphers: AES-256-GCM, ChaCha20-Poly1305
- Hash algorithms: SHA-256, SHA-3, BLAKE2
- Key derivation: HKDF, PBKDF2
- Random: /dev/random, /dev/urandom, getrandom syscall
- HMAC for IMA/EVM and network authentication

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

## 10. No io_uring or Zero-Copy Networking

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/io/uring.rs` (new) |
| **Status** | Partial |

**Resolution:** eventfd and timerfd implemented in Phase 13 (Syscalls 79-84).
io_uring and zero-copy networking remain open.

**Impact:** No high-performance async I/O interface. io_uring is the
highest-impact missing subsystem for network servers and storage workloads.

**Proposed Fix:**
- io_uring: submission queue, completion queue, SQE/CQE ring buffers
- Registered buffers and files for pinned memory
- Linked operations for dependent syscalls
- Zero-copy: sendfile, MSG_ZEROCOPY, splice

**Tracking:** `docs/roadmap.md` Phase 13

---

## 11. No Huge Pages, THP, NUMA, or KSM

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/memory/` |
| **Status** | Open |

**Impact:** 4KB pages only. TLB pressure is high on large-memory workloads.
No NUMA awareness means poor performance on multi-socket systems.

**Proposed Fix:**
- Huge pages: 2MB/1GB via hugetlbfs
- Transparent Huge Pages: automatic promotion/demotion
- NUMA: node-local allocation, memory policies, page migration
- KSM: same-page merging for deduplication
- Memory compression: zswap/zram
- Memory compaction: defragmentation for contiguous allocations

**Tracking:** `docs/roadmap.md` Phase 16

---

## 12. No Workqueues, Softirqs, or Tasklets

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/sync/workqueue.rs`, `kernel/src/softirq.rs` |
| **Status** | Partial |

**Resolution:** Workqueue and softirq implemented in Phase 12. WorkQueue provides
FIFO function-pointer dispatch. Softirq provides 8 named vectors with bitmask
tracking. Tasklets remain as a future enhancement.

---

## 13. No KASAN/KFENCE Memory Safety Detection

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/mm/kasan.rs` (new) |
| **Status** | Open |

**Impact:** No runtime memory error detection. Use-after-free, buffer
overflows, and uninitialized memory reads go undetected. Critical for
kernel development and CI.

**Proposed Fix:**
- KASAN: generic shadow memory for heap out-of-bounds and use-after-free
- KFENCE: low-overhead sampling-based detector for production
- Stack protector: canary-based stack overflow detection
- Memory poisoning: detect uninitialized memory reads

**Tracking:** `docs/roadmap.md` Phase 15

---

## 14. No Lockdep or Completion Variables

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/sync/lockdep.rs` (new) |
| **Status** | Partial |

**Resolution:** SeqLock and RwLock implemented in Phase 12. SeqLock provides
optimistic reader / exclusive writer. RwLock provides multiple-reader /
single-writer. Lockdep and completion variables remain as future enhancements.

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

## Summary

| # | Issue | Severity | Status |
|---|-------|----------|--------|
| 1 | ext4 writes are in-memory only | Medium | Partially Resolved |
| 2 | GP fault during fork/clone | Medium | Mitigated |
| 3 | No performance tracing (ftrace, kprobes) | High | Open |
| 4 | No memory compression (zswap/zram) | Medium | Open |
| 5 | No crash dump / reliability engineering | High | Open |
| 6 | No kernel crypto API | Medium | Open |
| 7 | No device driver PM / hotplug framework | Medium | Open |
| 8 | No hypervisor / virtualization support | Medium | Open |
| 9 | No userspace coreutils / POSIX utilities | Medium | Open |
| 10 | No io_uring or zero-copy networking | High | Partial |
| 11 | No huge pages, THP, NUMA, or KSM | High | Open |
| 12 | No workqueues, softirqs, or tasklets | High | Partial |
| 13 | No KASAN/KFENCE memory safety detection | High | Open |
| 14 | No lockdep or completion variables | Medium | Partial |
| 15 | No container runtime or OCI support | Medium | Open |
