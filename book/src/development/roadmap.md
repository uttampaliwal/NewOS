> For the full detailed roadmap with implementation plans, see [docs/roadmap.md](../../../docs/roadmap.md)

# Roadmap

## Completed Phases

| Phase | Milestone | Status |
|-------|-----------|--------|
| 0 | Repository scaffold, shared ABI, xtask automation | Complete |
| 1 | UEFI Boot, ACPI, PCIe, VirtIO-Net, NVMe, XHCI, DRM/KMS | Complete |
| 2 | VMAs, Demand Paging, mmap/munmap, Page Cache, Swap, ASLR, OOM | Complete |
| 3 | Process table, fork/exec/wait, VFS (tmpfs/ext4), Pipes, Sockets, Signals | Complete |
| 4 | POSIX Capabilities, Namespaces, Seccomp-BPF, LSM hooks, IMA/EVM | Complete |
| 5 | Package manager, SAT solver, TUF verification, staging/rollback | Complete |
| 6 | IPC Broker, structured logging, service unit manager | Complete |
| 7 | Wayland compositor, input routing, desktop session management | Complete |
| 8 | SMP, CFS scheduler, scheduler classes, cgroups v2, slab allocator | Complete |
| 10 | Epoll, futex, POSIX message queues, POSIX shared memory | Complete |

## Planned Phases

| Phase | Focus | Key Items |
|-------|-------|-----------|
| 9 | Networking Depth | IPv6, TCP congestion control, eBPF, nftables, TLS 1.3 |
| 11 | Memory & Storage | Huge pages, THP, NUMA, ext4 journaling, CoW filesystem |
| 12 | Scalability & Concurrency | RCU, per-CPU caches, workqueues, softirqs, seqlocks, lockdep |
| 13 | Async I/O & Zero-Copy | io_uring, zero-copy networking, eventfd, timerfd |
| 14 | Observability & Tracing | ftrace, kprobes, uprobes, perf, flamegraphs |
| 15 | Reliability Engineering | Crash dumps, watchdogs, KASAN/KFENCE, fault injection |
| 16 | Advanced Memory | Huge pages, THP, NUMA, memory compression, KSM |
| 17 | Security Hardening | CFI, KPTI, verified boot, Landlock, IOMMU |
| 18 | Containers | OCI runtime, OverlayFS, checkpoint/restore, device cgroups |
| 19 | Self-Hosting | POSIX compliance, coreutils, native toolchain |
| 20 | Virtualization | KVM, EPT/NPT, virtual devices |
| 21 | Distributed Systems | Service discovery, consensus, cluster scheduling |

## Current Maturity Scores

| Area | Score |
|------|-------|
| Architecture | 9/10 |
| Memory Management | 7.5/10 |
| Scheduling | 7.5/10 |
| IPC | 8.5/10 |
| Security | 6.5/10 |
| Networking | 5/10 |
| Observability | 4/10 |
| Reliability | 4/10 |
| Tooling | 7/10 |
| Scalability | 6/10 |

**Overall: ~7/10** — Exceptional for a hobby OS, approaching research OS level.
