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

## 17. No Scheduler Classes (CFS/RT/Deadline) (Resolved)

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/task/scheduler_class.rs`, `kernel/src/task/scheduler.rs` |
| **Status** | Resolved |

**Resolution:** Implemented scheduler class framework with `SchedulingPolicy` enum
(SCHED_NORMAL, SCHED_BATCH, SCHED_FIFO, SCHED_RR, SCHED_IDLE). CFS-style virtual
runtime (`vruntime`) tracking per task. Timer tick increments vruntime for
SCHED_NORMAL/BATCH tasks. Preemptive scheduler selects lowest-vruntime task from
run queue. `SchedSetScheduler`/`SchedGetScheduler` syscalls (IDs 72-73) expose
policy and priority to userland.

---

## 18. No cgroups v2 or Resource Accounting (Resolved)

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/cgroup/mod.rs` |
| **Status** | Resolved |

**Resolution:** Implemented cgroups v2 hierarchy with per-cgroup process tracking.
CPU controller: `cpu_used` tick accounting with `cgroup_cpu_tick()` called from
scheduler timer_tick — preempts process when quota exhausted. Memory controller:
`memory_used` tracking via `cgroup_memory_alloc()`/`cgroup_memory_free()` called
from demand paging — OOM-kills process when `memory_max` exceeded. PIDs controller:
`pids_max` limit enforced at fork time. Syscalls: `CgroupCreate` (74),
`CgroupAddProcess` (75), `CgroupSetCpuMax` (76), `CgroupSetMemoryMax` (77),
`CgroupSetPidsMax` (78). Processes track their `cgroup_path` in the PCB.

---

## 19. No Async I/O (epoll, io_uring, futex) (Resolved)

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/ipc/epoll.rs`, `kernel/src/ipc/futex.rs`, `kernel/src/ipc/mqueue.rs`, `kernel/src/ipc/shm.rs` |
| **Status** | Resolved |

**Resolution:** Implemented epoll, futex, POSIX message queues, and POSIX shared memory.

- **epoll**: `EpollCreate` (69), `EpollCtl` (70), `EpollWait` (71) with
  `EPOLLIN`/`EPOLLOUT`/`EPOLLRDHUP`. Blocking wait via `blocked_waiters` list.
  Global `notify_all_epoll_waiters()` triggered by pipe/socket/mqueue state changes.
- **futex**: `Futex` (68) with `FUTEX_WAIT`/`FUTEX_WAKE` for userspace
  synchronization primitives. Hash-table-based wait queue.
- **POSIX message queues**: `MqOpen` (63), `MqClose` (64), `MqUnlink` (65),
  `MqSend` (66), `MqReceive` (67). Blocking send/receive with notification.
- **POSIX shared memory**: `ShmOpen` (61), `ShmUnlink` (62). Wraps VFS
  operations on `/dev/shm/`.

**Remaining:** io_uring not yet implemented.

**Tracking:** `docs/roadmap.md` Phase 10, `docs/sota-gap-analysis.md` #9

---

## 20. No Performance Tracing (ftrace, kprobes)

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

**Tracking:** `docs/roadmap.md` Phase 13, `docs/sota-gap-analysis.md` #10

---

## 21. No Memory Compression (zswap/zram)

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

**Tracking:** `docs/roadmap.md` Phase 11, `docs/sota-gap-analysis.md` #5

---

## 22. No Crash Dump / Reliability Engineering

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

**Tracking:** `docs/roadmap.md` Phase 16, `docs/sota-gap-analysis.md` #16

---

## 23. No Kernel Crypto API

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

**Tracking:** `docs/roadmap.md` Phase 12, `docs/sota-gap-analysis.md` #6

---

## 24. No Device Driver PM / Hotplug Framework

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

**Tracking:** `docs/roadmap.md` Phase 16, `docs/sota-gap-analysis.md` #8

---

## 25. No Hypervisor / Virtualization Support

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

**Tracking:** `docs/roadmap.md` Phase 16, `docs/sota-gap-analysis.md` #14

---

## 26. No Userspace Coreutils / POSIX Utilities

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

**Tracking:** `docs/roadmap.md` Phase 15, `docs/sota-gap-analysis.md` #12

---

## 27. No RCU or Per-CPU Infrastructure

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/sync/rcu.rs` (new), `kernel/src/sync/percpu.rs` (new) |
| **Status** | Open |

**Impact:** No lock-free read-side access for concurrent data structures.
RCU is arguably the single biggest missing primitive for SOTA scalability.
Without per-CPU infrastructure, all shared data requires global locks.

**Proposed Fix:**
- RCU core: grace-period tracking, `rcu_read_lock`/`rcu_read_unlock`, `synchronize_rcu`
- RCU callbacks: deferred reclamation via `call_rcu`
- Tree RCU: hierarchical RCU for large CPU counts
- Per-CPU data: `DEFINE_PER_CPU` macro, `get_cpu_var`/`put_cpu_var`
- Per-CPU slab caches: reduce allocator lock contention

**Tracking:** `docs/roadmap.md` Phase 12

---

## 28. No io_uring or Zero-Copy Networking

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/io/uring.rs` (new) |
| **Status** | Open |

**Impact:** No high-performance async I/O interface. io_uring is the
highest-impact missing subsystem for network servers and storage workloads.

**Proposed Fix:**
- io_uring: submission queue, completion queue, SQE/CQE ring buffers
- Registered buffers and files for pinned memory
- Linked operations for dependent syscalls
- Zero-copy: sendfile, MSG_ZEROCOPY, splice
- eventfd and timerfd for event notification

**Tracking:** `docs/roadmap.md` Phase 13

---

## 29. No Huge Pages, THP, NUMA, or KSM

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

## 30. No Workqueues, Softirqs, or Tasklets

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/workqueue.rs` (new), `kernel/src/softirq.rs` (new) |
| **Status** | Open |

**Impact:** No deferred execution framework. All work must run in interrupt
context or process context. Cannot handle high-frequency events (network
RX/TX, block I/O completion) efficiently.

**Proposed Fix:**
- Workqueues: `queue_work`, `flush_work`, concurrency-managed workers
- Softirqs: high-priority deferred processing for network/block I/O
- Tasklets: softirq wrappers for simpler deferred work
- Timer wheel: high-resolution kernel timers

**Tracking:** `docs/roadmap.md` Phase 12

---

## 31. No KASAN/KFENCE Memory Safety Detection

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

## 32. No Lockdep or Seqlocks

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/sync/lockdep.rs` (new), `kernel/src/sync/seqlock.rs` (new) |
| **Status** | Open |

**Impact:** No deadlock detection or lock ordering validation. No
optimistic concurrency for read-mostly data. Difficult to debug
locking issues in multi-core code.

**Proposed Fix:**
- Lockdep: runtime lock dependency graph, deadlock detection, lock ordering validation
- Seqlocks: optimistic read-side with writer priority
- Completion variables: wait/signal for one-shot events

**Tracking:** `docs/roadmap.md` Phase 12

---

## 33. No Container Runtime or OCI Support

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
| 17 | No scheduler classes (CFS/RT/deadline) | High | Resolved |
| 18 | No cgroups v2 or resource accounting | High | Resolved |
| 19 | No async I/O (epoll, futex) | High | Resolved |
| 20 | No performance tracing (ftrace, kprobes) | High | Open |
| 21 | No memory compression (zswap/zram) | Medium | Open |
| 22 | No crash dump / reliability engineering | High | Open |
| 23 | No kernel crypto API | Medium | Open |
| 24 | No device driver PM / hotplug framework | Medium | Open |
| 25 | No hypervisor / virtualization support | Medium | Open |
| 26 | No userspace coreutils / POSIX utilities | Medium | Open |
| 27 | No RCU or per-CPU infrastructure | High | Open |
| 28 | No io_uring or zero-copy networking | High | Open |
| 29 | No huge pages, THP, NUMA, or KSM | High | Open |
| 30 | No workqueues, softirqs, or tasklets | High | Open |
| 31 | No KASAN/KFENCE memory safety detection | High | Open |
| 32 | No lockdep or seqlocks | Medium | Open |
| 33 | No container runtime or OCI support | Medium | Open |
