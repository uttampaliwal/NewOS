# SOTA Gap Analysis

This document records the state-of-the-art (SOTA) operating system assessment
performed on the `development` branch. It serves as the authoritative reference
for what must be built to compete conceptually with Linux, FreeBSD, Redox, and
Fuchsia.

---

## Current State Assessment

| Area                    | Current State            | SOTA Level |
| ----------------------- | ------------------------ | ---------- |
| Boot & Drivers          | Excellent hobby-OS level | 8/10       |
| Memory Management       | Good                     | 7/10       |
| POSIX Services          | Good                     | 7/10       |
| Security                | Very ambitious           | 8/10       |
| Scalability             | Limited                  | 4/10       |
| Multiprocessor Support  | Not visible              | 2/10       |
| Networking              | Basic                    | 3/10       |
| Storage                 | Basic                    | 4/10       |
| Performance Engineering | Minimal                  | 3/10       |
| Developer Ecosystem     | Good                     | 6/10       |
| Production Readiness    | Experimental             | 3/10       |

---

## Gap Details

### 1. SMP (Symmetric Multiprocessing)

**Current:** Phase 8 plans basic SMP with per-CPU scheduling and Local APIC.

**SOTA Requirements:**

- BSP/AP startup, Local APIC, IOAPIC, x2APIC, CPU hotplug
- Per-CPU run queues, CPU affinity, work stealing, load balancing
- NUMA-aware allocation, scheduler domains
- Synchronization: spinlocks, ticket locks, RwLocks, Seqlocks, RCU, lock-free structures

**Priority:** Critical

---

### 2. Preemptive Scheduler Improvements

**Current:** Preemptive round-robin scheduler only.

**SOTA Requirements:**

- Scheduler classes: RealTimeScheduler, FairScheduler (CFS-like), IdleScheduler, BatchScheduler
- CFS virtual runtime, priority inheritance, deadline scheduling
- CPU groups, scheduler domains

**Priority:** Critical

---

### 3. Networking Stack

**Current:** VirtIO-Net driver with basic TCP/IP via smoltcp (Phase 9 plans TCP/UDP).

**SOTA Requirements:**

- Layer 2: ARP, VLAN, bridges, bonding
- Layer 3: IPv4, IPv6, ICMP, routing tables
- Layer 4: TCP (CUBIC congestion), UDP, SCTP
- Layer 7: DNS resolver, DHCP client, HTTP stack
- Advanced: eBPF networking, net namespaces, nftables, zero-copy networking

**Priority:** Critical

---

### 4. Filesystems

**Current:** tmpfs, ext4 (read-only, in-memory only).

**SOTA Requirements:**

- Journaling: write-ahead log, recovery mode, crash consistency
- Additional FS: FAT32, exFAT, ISO9660, squashfs
- Advanced: CoW filesystem, snapshots, checksums, compression, encryption

**Priority:** High

---

### 5. Memory Management

**Current:** Demand paging, mmap, page cache, swap, ASLR/KASLR (Phase 8 plans NUMA).

**SOTA Requirements:**

- Huge pages: 2MB, 1GB, THP (Transparent Huge Pages)
- NUMA: node allocator, node migration, memory policies
- Advanced allocators: slab, SLUB, per-CPU caches, object caches
- Memory compression: zswap, zram

**Priority:** High

---

### 6. Security Hardening

**Current:** Namespaces, seccomp, capabilities, LSM, IMA/EVM, KASLR, stack canaries.

**SOTA Requirements:**

- Memory safety: CET shadow stacks, Control Flow Integrity, Pointer Authentication, SafeStack
- Kernel protection: KPTI, hardened usercopy, guard pages, init-on-alloc/free
- Sandboxing: Landlock, capability bounding, container runtime
- Cryptography: kernel crypto API, TPM integration, secure boot, measured boot

**Priority:** High

---

### 7. Process Isolation and Containers

**Current:** Namespaces (PID, mount, network, user). Phase 11 plans OCI runtime.

**SOTA Requirements:**

- cgroups v2: resource accounting, quotas, CPU/memory/IO controllers
- Container runtime: OCI images, `turnix run alpine`
- Container networking: veth pairs, bridge, overlay

**Priority:** High

---

### 8. Device Driver Model

**Current:** VirtIO, NVMe, USB, DRM/KMS as standalone drivers.

**SOTA Requirements:**

- Driver framework: Bus, Device, Driver, Probe, Remove, Suspend, Resume abstractions
- Power management: runtime PM, sleep states, hibernation
- Hotplug: USB hotplug, PCI hotplug

**Priority:** High

---

### 9. IPC System

**Current:** Pipes (ring-buffered), Unix domain sockets.

**SOTA Requirements:**

- Shared memory (shmget/shmat or mmap MAP_SHARED)
- POSIX message queues
- futexes (fast userspace mutexes)
- epoll (event notification)
- io_uring-like async I/O interface

**Priority:** High

---

### 10. Performance Engineering

**Current:** `dmesg`-based logging, 12 host-side benchmarks.

**SOTA Requirements:**

- Tracing: ftrace, ktrace, uprobes, kprobes, perf
- Profiling: flamegraphs, syscall tracing, scheduler tracing, lock contention analysis
- Benchmarks: context switch latency, IPC throughput, filesystem throughput, memory bandwidth

**Priority:** Critical

---

### 11. Graphics Stack

**Current:** DRM/KMS, Wayland compositor basics, double-buffering.

**SOTA Requirements:**

- GPU: OpenGL, Vulkan, GPU memory management, command submission
- Window system: Wayland protocol compatibility, hardware compositing, vsync, fractional scaling, accessibility

**Priority:** Medium

---

### 12. Userspace Ecosystem

**Current:** Package manager (tpkg), system services (init, shell, compositor).

**SOTA Requirements:**

- Core utilities: coreutils, grep, sed, awk, tar, ssh, curl
- Toolchain: clang, rustc, lld, gdb
- Package repos: binary repositories, signed packages, dependency visualization, reproducible builds

**Priority:** High

---

### 13. Self Hosting

**Current:** Phase 11 plans self-hosting toolchain.

**SOTA Requirements:**

- Phase A: Build kernel on Linux
- Phase B: Compile userspace on Turnix
- Phase C: Build Turnix on Turnix
- Phase D: Develop applications natively

**Priority:** Very High

---

### 14. Virtualization

**Current:** Phase 11 mentions KVM-style paravirtualization.

**SOTA Requirements:**

- Hypervisor: VT-x, AMD-V, nested paging, virtual devices
- Containers: OCI runtime, namespaces, cgroups

**Priority:** Medium

---

### 15. Distributed System Features

**Current:** No coverage.

**SOTA Requirements:**

- Service discovery, distributed filesystems, cluster scheduler, remote execution

**Priority:** Low

---

### 16. Reliability Engineering

**Current:** No coverage.

**SOTA Requirements:**

- Recovery: panic reports, crash dumps, watchdog, kernel checkpoints
- Fault injection: fail allocations, drop packets, inject I/O errors, simulate crashes

**Priority:** High

---

## Roadmap Mapping

These gaps are tracked in the expanded roadmap (`docs/roadmap.md`) as Phases 8-16:

| Phase | Focus | Gaps Addressed |
|---|---|---|
| 8 | SMP & Scalable Scheduler | #1, #2 |
| 9 | Networking Depth | #3 |
| 10 | Async I/O & Process Isolation | #7, #9 |
| 11 | Memory & Storage | #4, #5 |
| 12 | Security Hardening | #6 |
| 13 | Performance & Observability | #10 |
| 14 | Graphics & Desktop | #11 |
| 15 | Self-Hosting & Ecosystem | #12, #13 |
| 16 | Virtualization & Reliability | #14, #15, #16 |

---

## Priority Summary

The highest-impact areas for the next development cycle:

1. **SMP + Scalable Scheduler** — foundational for all parallelism
2. **cgroups v2 + Async I/O** — required for containers and production workloads
3. **Performance Tracing** — observability is prerequisite for all optimization
4. **Reliability Engineering** — crash dumps and fault injection for production use
