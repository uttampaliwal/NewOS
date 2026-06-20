# Turnix OS Development Roadmap

This document outlines the development trajectory of Turnix OS from foundational setup to a modern, secure, desktop-ready operating system.

---

## 🏗️ Phase 0: Foundation (Complete)
*   Repository layout scaffolding and subdirectories structures.
*   Setup guidelines for host environments.
*   Establishment of the shared ABI (`shared/abi`) and serial logging (`shared/serial`) crates.
*   Automation wrappers in `tools/xtask`.
*   Freestanding kernel crate skeleton (`kernel`).

---

## 🚀 Phase 1: Boot & Drivers (Complete)
*   **UEFI Loader**: Thin loader in `boot/uefi-loader` executing on nightly target `x86_64-unknown-uefi`.
*   **Handoff Contract**: Clean bootloader-to-kernel handoff with a strict `BootInfo` structure.
*   **PCI/PCIe ECAM**: Scanning system buses via ACPI MCFG mapping.
*   **ACPI AML**: Superblock parser (RSDP, XSDT, MCFG, DSDT/SSDT) and AML interpreter evaluation.
*   **VirtIO-Net**: Transitional/modern driver negotiation, virtqueue ring buffers, and MAC extraction.
*   **NVMe**: Block device driver supporting submission/completion queues and PRP memory transfers.
*   **XHCI USB**: Controller reset/initialization, port speed detection, and USB keyboard drivers.
*   **GPU DRM/KMS**: bochs-display framebuffers mapping for composites/compositors.

---

## 🧠 Phase 2: Memory Subsystem Maturity (Complete)
*   **Virtual Memory Areas (VMAs)**: Overlap conflict checking via sorted BTreeMaps.
*   **Demand Paging**: Page fault handlers performing frame allocation and zero-filling on access.
*   **Memory mapping syscalls**: Safe, bounds-checked implementation of `mmap` and `munmap`.
*   **LRU Page Cache**: File page mapping with dirty page tracking and writeback timers.
*   **Swap Manager**: Clock/LRU eviction of anonymous memory onto swap space.
*   **OOM Killer**: Score-based process killer (RSS and priority logic).
*   **ASLR & KASLR**: Random stack, heap, and load offsets for user space and kernel boot space.
*   **W^X Verification**: Strictly enforcing that no page table entries allow both Write and Execute permissions.

---

## 📂 Phase 3: POSIX-Compatible System Services (Complete)
*   **Process Table**: PCB tracking process state, signal masks, and file descriptors.
*   **Process Lifecycle**: Full implementation of `fork`, `exec` (ELF loading, ASLR, argument vectors), and parent `wait`/`waitpid` reaping.
*   **Virtual File System (VFS)**: Mount/unmount lifecycle, path resolution over mounts, and inode permissions.
*   **tmpfs & ext4 Backends**: In-memory tmpfs filesystem and ext4 read-write block device integration.
*   **IPC Mechanisms**: Ring-buffered pipes (with full blocking and `SIGPIPE` delivery) and Unix domain sockets (`AF_UNIX`).
*   **Console Redirection**: Stdin/stdout/stderr file descriptor routing and `dup`/`dup2` redirection.
*   **Init Daemon**: Services manifest manager parsing dependency orders (topological sort) and reaping orphaned children.

---

## 🛡️ Phase 4: Security Hardening (Complete)
*   **POSIX Capabilities**: standard 64-bit capability sets (`permitted`, `effective`, `inheritable`, `bounding`, `ambient`) to restrict privileged operations.
*   **Namespaces**: Mount, Network, PID (local init remapping), and User (UID/GID translation) namespaces.
*   **Seccomp-BPF**: System call filter inheritance using classic BPF evaluation.
*   **LSM Hook Framework**: Pluggable hooks with a default Unix DAC implementation.
*   **IMA/EVM**: Integrity Measurement Architecture log ring buffer and EVM checksum verifying file metadata.
*   **Stack Canaries**: Stack corruption checks placed at the base of task stacks.

---

## 📦 Phase 5: Package Management (In Progress)
*   **tpkg-format Crate**: Manifest serialization/deserialization and grammar validation.
*   **CDCL SAT Solver**: Dependency resolution package constraints and directed cycle detection.
*   **TUF Repository Client**: Signature verification of package metadata.
*   **Rollback Pipeline**: Install staging, ext4 copy-on-write snapshots, and transaction rollbacks on failures.

---

## ⚙️ Phase 6: System Services Layer (Planned)
*   **IPC Broker Daemon**: Async event loop routing socket method calls and signal broadcasts.
*   **Structured Logger**: Log rotation, HMAC-SHA256 seals, and serial logging integration.
*   **Service Manager**: Unit file configurations, restart policy backoffs, and socket activation.

---

## 🎨 Phase 7: Graphical Desktop Environment (Planned)
*   **Wayland Compositor**: Memory-mapped graphics framebuffers and compositor security checks.
*   **Input Router**: Routing keyboard/mouse inputs to active Compositor clients.
*   **Session Management**: Desktop service startup and user login handlers.
