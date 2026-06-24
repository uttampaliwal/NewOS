# Turnix OS Development Roadmap

This document outlines the development trajectory of Turnix OS from foundational
setup to a modern, secure, desktop-ready operating system. See
[SOTA Gap Analysis](sota-gap-analysis.md) for the full state-of-the-art
assessment.

---

## Phase 0: Foundation (Complete)
* Repository layout scaffolding and subdirectories structures.
* Setup guidelines for host environments.
* Establishment of the shared ABI (`shared/abi`) and serial logging (`shared/serial`) crates.
* Automation wrappers in `tools/xtask`.
* Freestanding kernel crate skeleton (`kernel`).

---

## Phase 1: Boot & Drivers (Complete)
* **UEFI Loader**: Thin loader in `boot/uefi-loader` executing on nightly target `x86_64-unknown-uefi`.
* **Handoff Contract**: Clean bootloader-to-kernel handoff with a strict `BootInfo` structure.
* **PCI/PCIe ECAM**: Scanning system buses via ACPI MCFG mapping.
* **ACPI AML**: Superblock parser (RSDP, XSDT, MCFG, DSDT/SSDT) and AML interpreter evaluation.
* **VirtIO-Net**: Transitional/modern driver negotiation, virtqueue ring buffers, and MAC extraction.
* **NVMe**: Block device driver supporting submission/completion queues and PRP memory transfers.
* **XHCI USB**: Controller reset/initialization, port speed detection, and USB keyboard drivers.
* **GPU DRM/KMS**: bochs-display framebuffers mapping for composites/compositors.

---

## Phase 2: Memory Subsystem Maturity (Complete)
* **Virtual Memory Areas (VMAs)**: Overlap conflict checking via sorted BTreeMaps.
* **Demand Paging**: Page fault handlers performing frame allocation and zero-filling on access.
* **Memory mapping syscalls**: Safe, bounds-checked implementation of `mmap` and `munmap`.
* **LRU Page Cache**: File page mapping with dirty page tracking and writeback timers.
* **Swap Manager**: Clock/LRU eviction of anonymous memory onto swap space.
* **OOM Killer**: Score-based process killer (RSS and priority logic).
* **ASLR & KASLR**: Random stack, heap, and load offsets for user space and kernel boot space.
* **W^X Verification**: Strictly enforcing that no page table entries allow both Write and Execute permissions.

---

## Phase 3: POSIX-Compatible System Services (Complete)
* **Process Table**: PCB tracking process state, signal masks, and file descriptors.
* **Process Lifecycle**: Full implementation of `fork`, `exec` (ELF loading, ASLR, argument vectors), and parent `wait`/`waitpid` reaping.
* **Virtual File System (VFS)**: Mount/unmount lifecycle, path resolution over mounts, and inode permissions.
* **tmpfs & ext4 Backends**: In-memory tmpfs filesystem and ext4 read-only block device integration.
* **IPC Mechanisms**: Ring-buffered pipes (with full blocking and `SIGPIPE` delivery) and Unix domain sockets (`AF_UNIX`).
* **Console Redirection**: Stdin/stdout/stderr file descriptor routing and `dup`/`dup2` redirection.
* **Init Daemon**: Services manifest manager parsing dependency orders (topological sort) and reaping orphaned children.

---

## Phase 4: Security Hardening (Complete)
* **POSIX Capabilities**: standard 64-bit capability sets (`permitted`, `effective`, `inheritable`, `bounding`, `ambient`) to restrict privileged operations.
* **Namespaces**: Mount, Network, PID (local init remapping), and User (UID/GID translation) namespaces.
* **Seccomp-BPF**: System call filter inheritance using classic BPF evaluation.
* **LSM Hook Framework**: Pluggable hooks with a default Unix DAC implementation and MAC framework.
* **IMA/EVM**: Integrity Measurement Architecture log ring buffer and EVM checksum verifying file metadata.
* **Stack Canaries**: Stack corruption checks placed at the base of task stacks.
* **FileCaps**: POSIX file capabilities read from xattr at exec time.
* **TPM 2.0 TIS Driver**: Probe, initialize, seal/unseal for secure key storage.

---

## Phase 5: Package Management (Complete)
* **tpkg-format Crate**: Manifest serialization/deserialization and grammar validation.
* **CDCL SAT Solver**: Dependency resolution package constraints and directed cycle detection.
* **TUF Repository Client**: Signature verification of package metadata.
* **Rollback Pipeline**: Install staging, ext4 copy-on-write snapshots, and transaction rollbacks on failures.
* **Network Fetcher**: HTTP client with SHA-256 verification and resume support.

---

## Phase 6: System Services Layer (Complete)
* **IPC Broker Daemon**: Async event loop routing socket method calls and signal broadcasts with credential-based authentication.
* **Structured Logger**: Log rotation, HMAC-SHA256 seals, and serial logging integration.
* **Service Manager**: Unit file configurations, restart policy backoffs, and socket activation.
* **Network Manager**: DHCP client, DNS resolver, static IP configuration, IPC integration.
* **Device Manager**: Hotplug event handling, driver rules, USB auto-mount, IPC exposure.
* **Kernel Network Stack**: smoltcp TCP/IP with VirtIO-Net driver, AF_INET/AF_INET6 socket syscalls, LAPIC-timer-driven poll loop.

---

## Phase 7: Graphical Desktop Environment (Complete)
* **Wayland Compositor**: Memory-mapped graphics framebuffers and compositor security checks.
* **Input Router**: Routing keyboard/mouse inputs to active Compositor clients.
* **Session Management**: Desktop service startup and user login handlers.
* **EDID Parser**: Display mode discovery from hardware.
* **Double-Buffering**: Tear-free rendering with VBlank synchronization.
* **Dynamic Resolution**: Runtime display mode changes.

---

## Phase 8: SMP & Scalable Scheduler (Complete)

**Goal:** Multi-core support with a production-grade scheduler.

### 8a. SMP Infrastructure
* **BSP/AP Startup**: Application processor bring-up via SIPI sequence
* **Local APIC**: Per-core timer, IPI (inter-processor interrupt)
* **Per-CPU Scheduling**: Flat Vec<Task> indexed by CPU ID

### 8b. Scheduler
* **Scheduler Class Framework**: Pluggable RealTime, Fair, Idle, Batch schedulers via `SchedulingPolicy` enum
* **EEVDF Scheduler**: Earliest Eligible Virtual Deadline First with 40 nice levels, weight-scaled vruntime, deadline-based preemption
* **Real-Time Scheduler**: FIFO and RR policies with static priorities
* **Scheduling Syscalls**: `SchedSetScheduler` (ID 72), `SchedGetScheduler` (ID 73)

### 8c. Resource Isolation
* **cgroups v2**: CPU quota enforcement via `cgroup_cpu_tick()`, memory limits with OOM-kill via `cgroup_memory_exceeded()`, PID limits
* **Cgroup Syscalls**: `CgroupCreate` (74), `CgroupAddProcess` (75), `CgroupSetCpuMax` (76), `CgroupSetMemoryMax` (77), `CgroupSetPidsMax` (78)
* **Slab Allocator**: Object caching for kernel allocations (pipe ring buffers)

---

## Phase 9: Networking Depth (Planned)

**Goal:** Full TCP/IP stack with modern networking features.

### 9a. Layer 2
* **ARP**: Address resolution protocol
* **VLAN**: 802.1Q virtual LAN tagging
* **Bridges**: Software network bridging
* **Bonding**: Link aggregation for redundancy/throughput

### 9b. Layer 3
* **IPv6**: Full dual-stack support
* **ICMPv6**: Neighbor discovery, path MTU
* **Routing Table**: Longest-prefix match, static and dynamic routes

### 9c. Layer 4
* **TCP Congestion Control**: CUBIC, Reno, BBR
* **SCTP**: Multi-homing transport (optional)

### 9d. Layer 7 Utilities
* **DNS Resolver**: Recursive resolver with cache and TTL
* **DHCP Client**: Full DHCPv4/v6 with lease management
* **HTTP/1.1 Client**: GET/POST with chunked transfer encoding
* **TLS 1.3**: rustls-based encrypted connections

### 9e. Advanced Networking
* **eBPF Networking**: Packet filtering and monitoring
* **Network Namespaces**: Full per-namespace routing and sockets
* **nftables**: Packet filtering framework
* **Zero-Copy Networking**: Sendfile, splice, MSG_ZEROCOPY

---

## Phase 10: Async I/O & Process Isolation (Complete)

**Goal:** High-performance I/O and resource isolation for containers.

### 10a. Async I/O
* **epoll**: Event notification for I/O multiplexing with `EPOLLIN`/`EPOLLOUT`/`EPOLLRDHUP`, blocking wait via `blocked_waiters`, global `notify_all_epoll_waiters()` from pipe/socket/mqueue
* **futexes**: Fast userspace mutexes (`FUTEX_WAIT`/`FUTEX_WAKE`) for synchronization
* **POSIX Message Queues**: `MqOpen`, `MqClose`, `MqUnlink`, `MqSend`, `MqReceive` with blocking and epoll notification
* **POSIX Shared Memory**: `ShmOpen`, `ShmUnlink` via `/dev/shm/` VFS

### 10b. Process Isolation
* **cgroups v2**: CPU, memory, and PIDs controllers with per-cgroup accounting
* **Resource Accounting**: `cpu_used`/`memory_used` tracking per cgroup
* **Resource Limits**: `cpu.max` (preempts on exhaustion), `memory.max` (OOM-kills on exceeded), `pids.max` (denies fork)

---

## Phase 11: Memory & Storage (Planned)

**Goal:** Advanced memory management and filesystem maturity.

### 11a. Memory Management
* **Slab Allocator**: ✅ Object caching for kernel allocations (pipe ring buffers)
* **Huge Pages**: 2MB and 1GB pages
* **Transparent Huge Pages (THP)**: Automatic huge page promotion
* **NUMA**: Node-aware allocation, migration, memory policies
* **Per-CPU Caches**: Reduce allocator lock contention
* **Memory Compression (zswap/zram)**: Compressed swap in RAM
* **Same-Page Merging (KSM)**: Deduplicate identical pages

### 11b. Filesystem Improvements
* **ext4 Journaling**: Write-ahead log, crash recovery
* **ext4 Block Allocator**: Inode and block allocation
* **FAT32**: Removable media support
* **squashfs**: Read-only compressed filesystem for packages
* **CoW Filesystem**: Copy-on-write with snapshots (future)

---

## Phase 12: Scalability & Concurrency (Complete)

**Goal:** Production-grade concurrency primitives for multi-core scalability.

### 12a. RCU (Read-Copy-Update)
* [x] **RCU Core**: Grace-period tracking, `rcu_read_lock`/`rcu_read_unlock`, `synchronize_rcu`
* [x] **RCU Callbacks**: Deferred reclamation via `call_rcu`, callback offloading
* [ ] **Tree RCU**: Hierarchical RCU for large CPU counts
* [ ] **SRCU**: Sleepable RCU for read-side critical sections that can block

### 12b. Per-CPU Infrastructure
* [ ] **Per-CPU Slab Caches**: Reduce allocator lock contention on hot paths
* [x] **Per-CPU Counters**: `PerCpuCounter`, `PerCpuAtomicCounter`, `PerCpuBool`
* [ ] **Per-CPU Data**: `DEFINE_PER_CPU` macro, `get_cpu_var`/`put_cpu_var`

### 12c. Workqueues
* [x] **Workqueue Framework**: Ring-buffer based function-pointer work queue with FIFO processing
* [ ] **Concurrency Managed Workqueues**: Auto-scaling worker threads
* [ ] **Bound Workqueues**: Per-CPU affinity for latency-sensitive work
* [ ] **Unbound Workqueues**: For offloadable, throughput-oriented work

### 12d. Deferred Execution
* [x] **Softirqs**: 8-vector bitmask-based deferred processing (Timer, NetTx, NetRx, Block, Tasklet, Scheduler, Security, Unused)
* [ ] **Tasklets**: Softirq wrappers for simpler deferred work
* [ ] **Timer Wheel**: High-resolution kernel timers

### 12e. Locking Primitives
* [x] **Seqlocks**: Optimistic concurrency for read-mostly data
* [x] **RwLock**: Multiple-reader / single-writer lock with try_read/try_write
* [ ] **Completion Variables**: Wait/signal for one-shot events
* [ ] **Lockdep**: Runtime deadlock detection and lock ordering validation
* [ ] **Priority Inheritance Futexes**: `FUTEX_LOCK_PI`/`FUTEX_UNLOCK_PI` for priority inversion avoidance

---

## Phase 13: Async I/O & Zero-Copy (Partial)

**Goal:** High-performance async I/O with zero-copy data paths.

### 13a. io_uring
* [x] **Submission Queue**: Ring buffer for batched syscall submission
* [x] **Completion Queue**: Ring buffer for async results
* [x] **SQE/CQE structs**: 12 operations (NOP, Read, Write, Close, Openat, Fsync, Statx, Send, Recv, PollAdd, PollRemove, Timeout)
* [ ] **Registered Buffers**: `IORING_REGISTER_BUFFERS` for pinned user memory
* [ ] **Registered Files**: `IORING_REGISTER_FILES` for fd table caching
* [ ] **Linked Operations**: Chain dependent operations
* [x] **Poll Integration**: `IORING_OP_POLL_ADD` for epoll-like efficiency

### 13b. Zero-Copy Networking
* [ ] **Sendfile**: Kernel-space file-to-socket transfer
* [ ] **MSG_ZEROCOPY**: Zero-copy send with completion notification
* [ ] **Splice / Tee**: Pipe-based zero-copy data movement
* [ ] **Buffer Sharing**: Shared page references between subsystems

### 13c. Event Notification
* [x] **eventfd**: Kernel-to-userspace event notification with epoll integration (Syscalls 79-81)
* [x] **timerfd**: Timer-based event notification, one-shot and periodic modes (Syscalls 82-84)

---

## Phase 14: Observability & Tracing (Planned)

**Goal:** Full visibility into kernel behavior for debugging and performance analysis.

### 14a. Tracing Framework
* **ftrace**: Function tracing, function_graph, events via tracefs
* **kprobes**: Dynamic kernel instrumentation at any function
* **uprobes**: Dynamic user-space instrumentation
* **tracefs**: Virtual filesystem for trace control and output

### 14b. Performance Counters
* **perf**: Hardware performance counter abstraction (PMU)
* **NMI Watchdog**: Non-maskable interrupt-based sampling
* **Callgraph Profiling**: Dwarf-based stack unwinding

### 14c. Analysis Tools
* **Lock Contention Analysis**: Spinlock/mutex wait tracking with owner identification
* **Scheduler Tracing**: Context switch latency, run queue depth, wake-up chains
* **Flamegraph Generation**: On-CPU and off-CPU flamegraphs from trace data
* **Syscall Latency Histograms**: Per-syscall entry-to-exit distributions

---

## Phase 15: Reliability Engineering (Planned)

**Goal:** Production-grade error detection, recovery, and debugging.

### 15a. Crash Dumps
* **Kdump-style Capture**: Reserved memory region for crash kernel
* **Panic Reports**: Structured logs with register dump, backtrace, oops decoding
* **Core Dump**: Full kernel memory dump for post-mortem analysis

### 15b. Watchdog & Lockup Detection
* **Hardware Watchdog**: HPET/LAPIC-based pre-panic countdown
* **Soft Lockup Detector**: Detect tasks holding CPU for extended periods
* **Hard Lockup Detector**: NMI-based detection of interrupts disabled too long
* **Hung Task Detector**: Detect tasks stuck in D state (uninterruptible sleep)

### 15c. Fault Injection
* **SLUB Error Injection**: Configurable failure points for kmalloc/kfree
* **I/O Error Injection**: Simulate disk/network failures
* **Network Loss/Delay Injection**: Simulate packet loss and latency
* **Failure Testing Framework**: Deterministic fault injection for CI

### 15d. Memory Safety Detection
* [x] **KASAN (Kernel Address Sanitizer)**: Heap out-of-bounds, use-after-free detection — shadow memory, poison/free poisoning, violation reporting
* **KFENCE (Kernel Electric Fence)**: Low-overhead sampling-based memory error detector
* **Stack Protector**: Canary-based stack overflow detection (stack canaries already in Phase 4)
* **Memory Poisoning**: Detect uninitialized memory reads

---

## Phase 16: Advanced Memory Management (Planned)

**Goal:** SOTA memory management with huge pages, NUMA, and compression.

### 16a. Huge Pages
* **Huge Pages (2MB/1GB)**: Explicit huge page allocation via `hugetlbfs`
* **Transparent Huge Pages (THP)**: Automatic promotion/demotion of 4KB pages
* **THP Defrag**: `khugepaged` for background compaction
* **Multi-size THP**: 16KB, 32KB, 64KB intermediate sizes

### 16b. NUMA
* **NUMA-Aware Allocation**: Node-local allocation with fallback
* **Memory Policies**: `set_mempolicy` (local, bind, interleave, preferred)
* **Page Migration**: Move pages between NUMA nodes for balancing
* **AutoNUMA**: Kernel-driven page placement based on access patterns

### 16c. Memory Compression
* **zswap**: Compressed write-back cache in front of swap device
* **zram**: Compressed block device in RAM
* **LZ4/ZSTD**: Configurable compression algorithms
* **Same-Page Merging (KSM)**: Deduplicate identical pages across processes

### 16d. Compaction & Fragmentation
* **Memory Compaction**: Defragmentation by moving pages to create contiguous regions
* **CMA (Contiguous Memory Allocator)**: Reserve contiguous regions for DMA
* **Buddy System Tuning**: Adjustable watermark ratios

---

## Phase 17: Security Hardening (Planned)

**Goal:** Production-grade security with hardware-backed protections.

### 17a. Verified Boot
* **UEFI Secure Boot Chain**: Signed loader → verified kernel
* **TPM PCR Extension**: Measure all boot components
* **dm-verity**: Root filesystem integrity verification
* **Secure Boot Policy**: Enforce signature requirements

### 17b. Kernel Self-Protection
* **Control Flow Integrity (CFI)**: Prevent ROP/JOP on indirect calls
* **KPTI (Kernel Page Table Isolation)**: Mitigate Meltdown
* **Hardened Usercopy**: Bounds check on copy_to/from_user
* **Guard Pages**: PROT_NONE between kernel structures
* **Init-on-Alloc/Free**: Zero memory to prevent info leaks
* **Kernel Lockdown**: Restrict /dev/mem, ACPI post-boot

### 17c. Advanced Sandboxing
* **Seccomp Notify**: User-space notification for dynamic policies
* **Landlock LSM**: Unprivileged sandboxing
* **Capability Bounding**: Permanent capability dropping

### 17d. Hardware Security
* **IOMMU**: DMA protection, device isolation for user-space drivers
* **Kernel Crypto API**: AES-GCM, ChaCha20-Poly1305, SHA-256/SHA-3
* **CET Shadow Stacks**: Hardware-backed control flow for user space

---

## Phase 18: Containers & Runtime Isolation (Planned)

**Goal:** Full container runtime support with image layering and checkpoint/restore.

### 18a. Container Runtime
* **OCI Runtime**: Container creation, lifecycle management
* **Overlay Filesystem**: Union mount for image layers
* **Image Layering**: Copy-on-write image storage
* **Device Cgroups**: Control device access per container

### 18b. Container Networking
* **veth Pairs**: Virtual ethernet pairs for container networking
* **Bridge Networking**: Linux bridge for container interconnection
* **Network Namespaces**: Full per-namespace routing and sockets

### 18c. Checkpoint/Restore
* **CRIU Integration**: Checkpoint/Restore In Userspace
* **Process Freezing**: Suspend container for consistent checkpoint
* **Memory Dump/Restore**: Serialize and restore process memory

### 18d. Seccomp Integration
* **Seccomp-BPF per Container**: Filter syscalls per container
* **Seccomp Notify**: Delegate policy decisions to supervisor

---

## Phase 19: Self-Hosting & Ecosystem (Planned)

**Goal:** Run Turnix natively and port real software.

### 19a. POSIX Compliance
* **LTP Integration**: Linux Test Project in CI
* **Open POSIX Test Suite**: Systematic conformance testing
* **Linux Syscall Layer**: Translate Linux syscalls to Turnix API
* **Binary Compatibility**: Run unmodified Linux binaries

### 19b. Core Utilities
* **coreutils Port**: cat, ls, cp, mv, rm, mkdir, chmod, chown, ps, top, df
* **Shell Porting**: bash/dash compatibility for build scripts
* **Text Processing**: grep, sed, awk, sort, uniq, wc

### 19c. Toolchain
* **clang/lld on Turnix**: Native C/C++ compiler
* **rustc on Turnix**: Native Rust compiler
* **make/cargo on Turnix**: Native build systems

### 19d. Package Ecosystem
* **Binary Repositories**: Pre-built packages for common software
* **Source Packages**: Build from source with patches
* **Reproducible Builds**: Deterministic compilation

---

## Phase 20: Virtualization (Planned)

**Goal:** Hardware-accelerated VM support.

* **VT-x / AMD-V**: Hardware virtualization for guest VMs
* **Nested Paging (EPT/NPT)**: Guest memory isolation
* **Virtual Devices**: VirtIO net, block, console for guests
* **/dev/kvm**: Userspace hypervisor interface
* **Live Migration**: Kernel state save/restore for VM migration

---

## Phase 21: Distributed Systems (Planned)

**Goal:** Multi-node and network-transparent services.

* **Service Discovery**: Dynamic service registration and lookup
* **Distributed Filesystems**: Network-transparent file access
* **Cluster Scheduler**: Multi-node workload distribution
* **Remote Execution**: Execute tasks across network nodes
* **Consensus Protocols**: Raft/Paxos for coordination (future)

---

## Known Limitations

See [KNOWN_ISSUES.md](../KNOWN_ISSUES.md) for current limitations:
* ext4 writes are in-memory only (no block allocator, no journal)
* No io_uring or zero-copy networking
* No ftrace/kprobes/perf observability
* No crash dump / reliability engineering
* No huge pages, NUMA, or memory compression
* No seccomp notify, Landlock, or verified boot
* No container runtime or OCI support
* No kernel crypto API
* No device driver PM / hotplug framework

See [SOTA Gap Analysis](sota-gap-analysis.md) for the full state-of-the-art
assessment and gap details.
