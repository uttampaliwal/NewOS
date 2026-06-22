# Turnix OS Development Roadmap

This document outlines the development trajectory of Turnix OS from foundational setup to a modern, secure, desktop-ready operating system.

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

## Phase 8: SMP & Hardware Scaling (Planned)

* **SMP Support**: Multi-core boot with per-CPU scheduling.
* **APIC (Advanced Programmable Interrupt Controller)**: Local APIC timer per core, I/O APIC for device interrupts.
* **NUMA Awareness**: Memory allocation policies for NUMA topologies.
* **Lock-Free Data Structures**: Per-CPU run queues, atomic counters for shared state.
* **CPU Affinity**: Process-to-core binding via `sched_setaffinity`.

---

## Phase 9: Networking Stack Maturity (Planned)

* **TCP/IP Stack Polish**: Connection state machine, congestion control (CUBIC).
* **DNS Resolver**: Recursive resolver with cache and TTL support.
* **HTTP/1.1 Client**: GET/POST with chunked transfer encoding.
* **TLS 1.3**: rustls-based TLS for encrypted connections.
* **Socket Options**: SO_REUSEADDR, TCP_NODELAY, keepalive.
* **Network Namespace Isolation**: Full per-namespace routing tables and socket bindings.

---

## Phase 10: Desktop Environment Polish (Planned)

* **Wayland Compositor Improvements**: Multi-monitor support, window decorations.
* **GPU Acceleration**: VirtIO-GPU 3D, Vulkan compute shaders.
* **Package Repository**: Public tpkg repository with signed packages.
* **Font Rendering**: FreeType integration, subpixel antialiasing.
* **Audio Stack**: ALSA-compatible audio via VirtIO-SND.
* **Clipboard**: Wayland clipboard protocol implementation.

---

## Phase 11: Self-Hosting & Ecosystem (Planned)

* **Self-Hosting Toolchain**: GCC/Rust cross-compiler running on Turnix.
* **Rust Compiler Port**: Build Rust crates natively on Turnix.
* **Native Development Environment**: Text editor, debugger, build system.
* **POSIX Compliance**: Expanded POSIX syscall coverage for software compatibility.
* **Container Support**: OCI-compatible container runtime using namespaces.
* **Virtualization**: KVM-style paravirtualization for running VMs.

---

## Phase 12: Security Hardening (Planned)

* **CET Shadow Stacks**: Hardware-backed control flow integrity for user space.
* **Control Flow Integrity (CFI)**: Forward-edge CFI for indirect calls in kernel and user space.
* **KPTI (Kernel Page Table Isolation)**: Separate user/kernel page tables to mitigate Meltdown-class attacks.
* **Hardened Usercopy**: Bounds checking on all copy_to_user/copy_from_user operations.
* **Guard Pages**: PROT_NONE guard pages between kernel stacks, heap, and mmap regions.
* **Init-on-Alloc/Free**: Memory initialization on allocation and zeroing on free to prevent use-after-free info leaks.
* **Landlock LSM**: Unprivileged sandboxing via BPF-like policy enforcement.
* **Capability Bounding**: Drop capabilities permanently via prctl PR_CAPBSET_DROP.
* **Kernel Crypto API**: AES-GCM, ChaCha20-Poly1305, SHA-256/SHA-3 for in-kernel cryptographic operations.
* **Secure Boot Chain**: UEFI Secure Boot -> signed kernel -> dm-verity for root filesystem.
* **Measured Boot**: TPM PCR extension for boot chain integrity measurement.

---

## Phase 13: Performance & Observability (Planned)

* **ftrace Framework**: Function tracing, function graph tracing, event tracing via tracefs.
* **kprobes & uprobes**: Dynamic instrumentation points in kernel and user space.
* **perf Integration**: Hardware performance counter access, PMU abstraction.
* **Lock Contention Analysis**: Spinlock/mutex wait time tracking, contention histograms.
* **Scheduler Tracing**: Context switch latency, run queue depth, wakeup-to-running time.
* **Flamegraph Generation**: Off-CPU and on-CPU flamegraph support from trace data.
* **Syscall Latency Tracing**: Per-syscall histogram of entry-to-exit time.
* **Context Switch Benchmarks**: Quantified measurements of voluntary/involuntary context switches.
* **IPC Throughput Benchmarks**: Pipe, socket, and shared memory throughput and latency.
* **Filesystem Benchmarks**: Metadata-heavy, sequential, and random I/O benchmarks.
* **Memory Bandwidth Benchmarks**: Sequential read/write/copy throughput measurements.

---

## Phase 14: Graphics & Desktop Polish (Planned)

* **OpenGL ES 3.0**: Mesa/Gallium software renderer or VirtIO-GPU 3D passthrough.
* **Vulkan 1.0**: WSI (Window System Integration) for Wayland, command buffer submission.
* **GPU Memory Management**: GEM/TTM-style buffer object management, GPU page tables.
* **Wayland Protocol Compatibility**: xdg-shell, xdg-decoration, layer-shell, presentation-time.
* **Hardware Compositing**: DRM atomic modesetting, overlay planes, cursor planes.
* **VSync & Frame Pacing**: Presentation-time feedback, adaptive sync, triple buffering.
* **Fractional Scaling**: Per-output scale factors, viewport transforms.
* **Accessibility**: High contrast, screen reader protocol, keyboard navigation.

---

## Phase 15: Self-Hosting & Ecosystem (Planned)

* **coreutils Port**: cat, ls, cp, mv, rm, mkdir, chmod, chown, ps, top, df, du, etc.
* **Shell Porting**: bash or dash compatibility layer for build scripts.
* **Toolchain on Turnix**: clang, lld, rustc running natively.
* **Native Build System**: make, cmake, or cargo running on Turnix.
* **Package Repositories**: Public tpkg package server with signed metadata.
* **Reproducible Builds**: Deterministic compilation, buildID verification.
* **Binary Repositories**: Pre-built packages for common development tools.
* **POSIX Compliance Expansion**: Additional syscalls for software compatibility (semaphores, message queues, shared memory).

---

## Phase 16: Virtualization & Reliability (Planned)

* **VT-x / AMD-V Support**: Hardware virtualization for running guest VMs.
* **Nested Paging**: EPT/NPT for guest memory isolation.
* **Virtual Devices**: VirtIO paravirtual devices for guests (net, block, console, input).
* **OCI Container Runtime**: Container creation, namespaces, cgroups, rootfs management.
* **Crash Dumps**: Kernel panic → coredump pipeline, post-mortem analysis.
* **Watchdog Timer**: Hardware watchdog for automatic reset on hang detection.
* **Fault Injection Framework**: Configurable fault injection for allocations, I/O, and network.
* **Kernel Checkpoints**: Save/restore kernel state for live migration or rollback.
* **Panic Reports**: Structured panic logs with register state, backtrace, and oops decoding.

---

## Known Limitations

See [KNOWN_ISSUES.md](../KNOWN_ISSUES.md) for current limitations:
* ext4 writes are in-memory only (no block allocator, no journal)

See [SOTA Gap Analysis](sota-gap-analysis.md) for the full state-of-the-art
assessment and gap details.
