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

## Known Limitations

See [KNOWN_ISSUES.md](../KNOWN_ISSUES.md) for current limitations:
* ext4 writes are in-memory only (no block allocator, no journal)
