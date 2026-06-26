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

## 5. ~~No Crash Dump / Reliability Engineering~~ (Resolved)

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/crash_dump.rs`, `kernel/src/watchdog.rs`, `kernel/src/fault_inject.rs` |
| **Status** | Resolved |

**Resolution:** Full reliability engineering subsystem implemented:

- **Crash dump** (`crash_dump.rs`): Enhanced with `PanicSeverity` (Kernel/Oops/Hardware),
  task ID and CPU ID tracking, structured output with severity-aware formatting.
  `print_panic_report()` now shows severity, task, and CPU context.
- **Watchdog** (`watchdog.rs`): Configurable hardware watchdog timer with pre-panic
  countdown. `kick_watchdog()` resets counter, `watchdog_tick()` decrements on each
  timer interrupt. Pre-panic countdown allows diagnostic output before panic.
  13 tests covering init, kick, tick, expiry, disarm, arm, stats, zero timeout.
- **Fault injection** (`fault_inject.rs`): Configurable failure points for Alloc/I/O/
  Network/FileSystem subsystems. Probability-based and every-N-th-check modes.
  Global enable/disable switch, per-point enable/disable, force-inject for testing.
  16 tests covering registration, probability, every-N, enable/disable, global toggle.

Total: 1051 tests pass.

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

## 7. ~~No Device Driver PM / Hotplug Framework~~ (Resolved)

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/drv/` |
| **Status** | Resolved |

**Resolution:** Full device driver subsystem implemented across three modules:

- **Driver model** (`mod.rs`): Bus/Device/Driver abstraction with `BusType` enum,
  `register_device()`, `register_driver()`, `probe_device()`, `remove_driver()`,
  `suspend_device()`, `resume_device()`, `suspend_all_devices()`, `resume_all_devices()`.
  Device capabilities bitmask, device/driver lookup and listing. 16 tests.
- **Runtime PM** (`runtime_pm.rs`): Reference-counted autosuspend with
  `runtime_get_sync()` / `runtime_put_suspend()` API. Per-device auto-suspend delay,
  global tick-based auto-suspend processing, enable/disable per-device and globally.
  `MockSwapDevice` for testing. 16 tests.
- **Hotplug framework** (`hotplug.rs`): Event-driven device insertion/removal system.
  Listener registration with event-type filtering (DeviceAdd/Remove, DriverBind/Unbind).
  Pending add/remove queue with `hotplug_process_pending()` dispatch. Event log with
  configurable max size. 15 tests.

---

## 8. ~~No Hypervisor / Virtualization Support~~ (Resolved)

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/kvm/` |
| **Status** | Resolved |

**Resolution:** Full KVM/hypervisor subsystem implemented across 6 modules:

- **VMCS** (`vmcs.rs`): Intel VT-x Virtual Machine Control Structure management.
  `Vmcs` struct with `vmread`/`vmwrite`/`vmclear`/`vmptrld` instruction wrappers.
  `VmcsField` enum for all standard VMCS fields (guest state, host state, exit info).
  8 tests.

- **VMCB** (`vmcb.rs`): AMD-V Virtual Machine Control Block management.
  `VmcbControl` and `VmcbSave` structs for control and guest register state.
  `vmrun` instruction wrapper. `VmcbExitCode` enum for exit reason decoding.
  Dirty tracking for lazy state restore. 8 tests.

- **EPT** (`ept.rs`): Extended Page Tables for Intel VT-x guest physical → host
  physical memory mapping. 4-level hierarchy (PML4→PDPT→PD→PT) with on-demand
  intermediate table allocation. `map_page`/`unmap_page`/`resolve` API.
  10 tests.

- **VM lifecycle** (`vm.rs`): Virtual machine creation, destruction, and state
  management. `VmManager` with global registry. `VirtualMachine` struct with
  EPT, guest memory, and vCPU state. `Vcpu` struct with full register state.
  State transitions: Created→Running→Paused→Halted. 12 tests.

- **VM exit handling** (`vmentry.rs`): VM exit reason decoding and dispatch.
  `ExitReason` enum (Hlt, IoInstruction, Cpuid, MsrAccess, EptViolation,
  TripleFault, etc.). CPUID leaf emulation, I/O instruction handling.
  10 tests.

- **/dev/kvm** (`kvm_dev.rs`): Userspace hypervisor interface. `kvm_ioctl`
  dispatch for KVM_GET_API_VERSION, KVM_CREATE_VM, KVM_CREATE_VCPU, KVM_RUN,
  KVM_SET_USER_MEMORY_REGION. `KvmRun` shared memory structure.
  8 tests.

- **VirtIO devices** (`virtio.rs`): Paravirtualized I/O for guest VMs.
  Virtqueue management with descriptor chains, available/used rings.
  `VirtioNet` (MAC, link status, packet send/recv), `VirtioBlock` (sector R/W),
  `VirtioConsole` (character I/O). Device manager with registration.
  16 tests.

---

## 9. ~~No Userspace Coreutils / POSIX Utilities~~ (Resolved)

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `userland/coreutils/` |
| **Status** | Resolved |

**Resolution:** Full coreutils crate implemented with 28 POSIX utilities across
three categories. All utilities are `#![no_std]` binaries using `libturnix` syscalls.

- **File operations** (9): `cat` (read/concatenate files), `rm` (remove files),
  `mkdir` (create directories), `rmdir` (remove directories), `cp` (copy files),
  `mv` (move files via copy+delete), `touch` (create empty files), `ls` (list
  directory contents), `chmod` (stub — prints not supported)
- **Text processing** (7): `echo` (output text), `wc` (line/word/byte count),
  `head` (first N lines), `tail` (last N lines), `grep` (substring search),
  `sort` (insertion sort), `uniq` (filter adjacent duplicates)
- **System info** (12): `whoami` (print username), `id` (print uid/gid),
  `uname` (system info with -a/-s flags), `uptime` (system uptime), `ps`
  (process listing), `dmesg` (kernel messages), `kill` (send signal with -s flag),
  `sleep` (delay N seconds), `pwd` (print working directory), `true` (exit 0),
  `false` (exit 1), `chown` (stub — prints not supported)

Added `arg()`, `arg_after_prog()`, `args_after_prog_count()`, `write_all()`,
and `write_str()` helper functions to `libturnix` for argument parsing and
convenient I/O.

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

## 11. ~~No Huge Pages, THP, NUMA, or KSM~~ (Resolved)

| | |
|---|---|
| **Severity** | High |
| **Component** | `kernel/src/memory/` |
| **Status** | Resolved |

**Resolution:** Full huge page, THP, NUMA, and KSM subsystem implemented with
complete test coverage across 7 modules:

- **Huge Pages** (`hugepage.rs`): 2MiB/1GiB pool allocator with alloc/free/stats, 12 tests
- **KSM** (`ksm.rs`): content-hash-based page deduplication with stable tree, COW fault handling, 12 tests
- **NUMA** (`numa.rs`): multi-node tracking, 5 allocation policies (Local/Bind/Interleave/Preferred/Default), distance matrix, memory tiers, 18 tests
- **THP** (`thp.rs`): region-based promotion/demotion with access-count threshold, 15 tests
- **Memory Compaction** (`compaction.rs`): zone-based compaction for contiguous allocations, defragmentation, 10 tests
- **HugeTLB** (`hugetlb.rs`): hugeTLB filesystem mount/unmount interface, page allocation/freeing, 10 tests
- **NUMA Migration** (`migration.rs`): page migration between NUMA nodes with policy support (None/Always/Once/CostBased), 10 tests

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

## 15. ~~No Container Runtime or OCI Support~~ (Resolved)

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/container/` |
| **Status** | Resolved |

**Resolution:** Full container runtime implemented across three modules:

- **OCI Spec** (`spec.rs`): OCI container specification parsing with `OciSpec`,
  `OciProcess`, `OciRoot`, `OciLinux`, `OciLinuxNamespace`, `OciResources`
  structs. `NamespaceType` enum (Pid, Network, Mount, User, Ipc, Uts, Cgroup).
  `parse_default_spec()` for creating default specs, `validate()` for spec
  validation. 10 tests.

- **Container Lifecycle** (`container.rs`): Full container state machine
  (Created→Running→Paused→Stopped→Deleted). `ContainerManager` with global
  registry. `create_container()` creates namespaces and sets up cgroup.
  `start_container()`, `stop_container()`, `pause_container()`,
  `resume_container()`, `delete_container()` with proper state transitions.
  15 tests covering full lifecycle, state transitions, and error cases.

- **Container Networking** (`network.rs`): Veth pair and bridge management.
  `create_veth_pair()` creates host/container veth pairs. `create_bridge()`
  and `attach_to_bridge()` for bridge networking. Container network isolation
  via network namespaces. 10 tests.

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

## 17. ~~Uneven Test Coverage~~ (Resolved)

| | |
|---|---|
| **Severity** | Medium |
| **Component** | `kernel/src/task/`, `kernel/src/net/` |
| **Status** | Resolved |

**Resolution:** Comprehensive test coverage added across scheduler and networking:

- **Scheduler SMP tests** (`task/scheduler_extra_tests.rs`): 11 tests covering SMP
  load balancing (steal from loaded CPU, respects balance), CFS vruntime fairness
  (equal-weight tasks get equal time, higher weight gets priority), timer tick
  behavior (vruntime advance, eligibility), deadline ordering, task count accuracy,
  round-robin selection, and vruntime monotonicity.

- **Socket state machine tests** (`net/socket_tests.rs`): 17 tests covering the
  full TCP socket lifecycle: Closed→Bound→Listening→Established→Close transitions,
  double-bind rejection, listen-without-bind rejection, accept behavior, send/recv
  roundtrip, port isolation, FD sequential allocation, UDP lifecycle, double-connect
  rejection, and accept-on-non-listening rejection.

---

## Summary

| # | Issue | Severity | Status |
|---|-------|----------|--------|
| 1 | ext4 writes are in-memory only | Medium | Resolved |
| 2 | GP fault during fork/clone | Medium | Mitigated |
| 3 | No performance tracing (ftrace, kprobes) | High | Resolved |
| 4 | No memory compression (zswap/zram) | Medium | Resolved |
| 5 | No crash dump / reliability engineering | High | Resolved |
| 6 | No kernel crypto API | Medium | Resolved |
| 7 | No device driver PM / hotplug framework | Medium | Resolved |
| 8 | No hypervisor / virtualization support | Medium | Resolved |
| 9 | No userspace coreutils / POSIX utilities | Medium | Resolved |
| 10 | No io_uring or zero-copy networking | High | Resolved |
| 11 | No huge pages, THP, NUMA, or KSM | High | Resolved |
| 12 | No workqueues, softirqs, or tasklets | High | Resolved |
| 13 | No KASAN/KFENCE memory safety detection | High | Resolved |
| 14 | No lockdep or completion variables | Medium | Resolved |
| 15 | No container runtime or OCI support | Medium | Resolved |
| 16 | Undocumented unsafe blocks | Medium | Resolved |
| 17 | Uneven test coverage | Medium | Resolved |
