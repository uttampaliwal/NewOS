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

## Phase 8: SMP & Scalable Scheduler (Planned)

**Goal:** Multi-core support with a production-grade scheduler.

### 8a. SMP Infrastructure
* **BSP/AP Startup**: Application processor bring-up via SIPI sequence
* **Local APIC**: Per-core timer, IPI (inter-processor interrupt)
* **I/O APIC**: Device interrupt routing to cores
* **x2APIC**: Extended APIC for large core counts
* **CPU Hotplug**: Dynamic CPU online/offline

### 8b. Scheduler
* **Scheduler Class Framework**: Pluggable RealTime, Fair, Idle, Batch schedulers
* **CFS/EEVDF Scheduler**: Virtual runtime tracking, red-black tree, fair scheduling
* **Real-Time Scheduler**: FIFO and RR policies with static priorities
* **Priority Inheritance**: Prevent priority inversion on mutexes
* **CPU Affinity**: `sched_setaffinity` / `sched_getaffinity`
* **Scheduler Domains**: NUMA-aware load balancing

### 8c. Synchronization Primitives
* **Ticket Locks**: Fair spinlocks with FIFO ordering
* **RwLocks**: Reader-writer locks for read-heavy paths
* **Seqlocks**: Optimistic concurrency for read-mostly data
* **RCU (Read-Copy-Update)**: Lock-free read-side access
* **Per-CPU Data**: CPU-local storage to avoid contention

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

## Phase 10: Async I/O & Process Isolation (Planned)

**Goal:** High-performance I/O and resource isolation for containers.

### 10a. Async I/O
* **epoll**: Event notification for I/O multiplexing (EPOLLIN/EPOLLOUT/EPOLLRDHUP)
* **io_uring**: Submission/completion queue ring buffers for async I/O
* **futexes**: Fast userspace mutexes for synchronization
* **eventfd**: Event notification for I/O integration

### 10b. Process Isolation
* **cgroups v2**: CPU, memory, I/O, PIDs controllers
* **Resource Accounting**: Per-cgroup usage tracking
* **Resource Limits**: cpu.max, memory.max, io.max, pids.max
* **Container Integration**: OCI runtime foundation

---

## Phase 11: Memory & Storage (Planned)

**Goal:** Advanced memory management and filesystem maturity.

### 11a. Memory Management
* **Huge Pages**: 2MB and 1GB pages
* **Transparent Huge Pages (THP)**: Automatic huge page promotion
* **NUMA**: Node-aware allocation, migration, memory policies
* **Slab/SLUB Allocator**: Object caching for frequent allocations
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

## Phase 12: Security Hardening (Planned)

**Goal:** Production-grade security with hardware-backed protections.

### 12a. Verified Boot
* **UEFI Secure Boot Chain**: Signed loader → verified kernel
* **TPM PCR Extension**: Measure all boot components
* **dm-verity**: Root filesystem integrity verification
* **Secure Boot Policy**: Enforce signature requirements

### 12b. Kernel Self-Protection
* **Control Flow Integrity (CFI)**: Prevent ROP/JOP on indirect calls
* **KPTI (Kernel Page Table Isolation)**: Mitigate Meltdown
* **Hardened Usercopy**: Bounds check on copy_to/from_user
* **Guard Pages**: PROT_NONE between kernel structures
* **Init-on-Alloc/Free**: Zero memory to prevent info leaks
* **Kernel Lockdown**: Restrict /dev/mem, ACPI post-boot

### 12c. Advanced Sandboxing
* **Seccomp Notify**: User-space notification for dynamic policies
* **Landlock LSM**: Unprivileged sandboxing
* **Capability Bounding**: Permanent capability dropping

### 12d. Hardware Security
* **IOMMU**: DMA protection, device isolation for user-space drivers
* **Kernel Crypto API**: AES-GCM, ChaCha20-Poly1305, SHA-256/SHA-3
* **CET Shadow Stacks**: Hardware-backed control flow for user space

---

## Phase 13: Performance & Observability (Planned)

**Goal:** Full visibility into system behavior and performance.

### 13a. Tracing Framework
* **ftrace**: Function tracing, function graph, events via tracefs
* **kprobes**: Dynamic kernel instrumentation points
* **uprobes**: Dynamic user-space instrumentation
* **perf**: Hardware performance counters (PMU)

### 13b. Analysis Tools
* **Lock Contention Analysis**: Spinlock/mutex wait tracking
* **Scheduler Tracing**: Context switch latency, run queue depth
* **Flamegraph Generation**: On-CPU and off-CPU flamegraphs
* **Syscall Latency**: Per-syscall entry-to-exit histograms

### 13c. Benchmarks
* **Context Switch Latency**: Voluntary/involuntary measurements
* **IPC Throughput**: Pipe, socket, shared memory
* **Filesystem Throughput**: Metadata, sequential, random I/O
* **Memory Bandwidth**: Read/write/copy throughput

---

## Phase 14: Graphics & Desktop Polish (Planned)

**Goal:** Hardware-accelerated graphics with Wayland protocol compatibility.

* **OpenGL ES 3.0**: Mesa/Gallium renderer or VirtIO-GPU 3D
* **Vulkan 1.0**: WSI for Wayland, command buffer submission
* **GPU Memory Management**: GEM/TTM buffer objects
* **Wayland Protocols**: xdg-shell, xdg-decoration, layer-shell
* **Hardware Compositing**: DRM atomic modesetting, overlay planes
* **VSync & Frame Pacing**: Presentation-time, adaptive sync
* **Fractional Scaling**: Per-output scale factors
* **Accessibility**: Screen reader protocol, keyboard navigation

---

## Phase 15: Self-Hosting & Ecosystem (Planned)

**Goal:** Run Turnix natively and port real software.

### 15a. POSIX Compliance
* **LTP Integration**: Linux Test Project in CI
* **Open POSIX Test Suite**: Systematic conformance testing
* **Linux Syscall Layer**: Translate Linux syscalls to Turnix API
* **Binary Compatibility**: Run unmodified Linux binaries

### 15b. Core Utilities
* **coreutils Port**: cat, ls, cp, mv, rm, mkdir, chmod, chown, ps, top, df
* **Shell Porting**: bash/dash compatibility for build scripts
* **Text Processing**: grep, sed, awk, sort, uniq, wc

### 15c. Toolchain
* **clang/lld on Turnix**: Native C/C++ compiler
* **rustc on Turnix**: Native Rust compiler
* **make/cargo on Turnix**: Native build systems

### 15d. Package Ecosystem
* **Binary Repositories**: Pre-built packages for common tools
* **Source Packages**: Build from source with patches
* **Reproducible Builds**: Deterministic compilation

---

## Phase 16: Virtualization & Reliability (Planned)

**Goal:** VM support and production reliability.

### 16a. Virtualization
* **VT-x / AMD-V**: Hardware virtualization for guest VMs
* **Nested Paging (EPT/NPT)**: Guest memory isolation
* **Virtual Devices**: VirtIO net, block, console for guests
* **/dev/kvm**: Userspace hypervisor interface
* **OCI Container Runtime**: Container creation, namespaces, cgroups

### 16b. Reliability Engineering
* **Crash Dumps**: Kernel panic → coredump → post-mortem
* **Watchdog Timer**: Hardware watchdog for hang detection
* **Fault Injection**: Configurable failures for alloc, I/O, network
* **Kernel Checkpoints**: Save/restore for live migration
* **Panic Reports**: Structured logs with backtrace and oops decoding

---

## Phase 17: Distributed Systems (Planned)

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

See [SOTA Gap Analysis](sota-gap-analysis.md) for the full state-of-the-art
assessment and gap details.
