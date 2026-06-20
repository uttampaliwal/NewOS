# Turnix OS Architecture

Turnix is a SOTA, Rust-first operating system designed to combine the safety of type-safe languages with the power and control of POSIX-like environments. It is structured as a modular monolith where subsystems are cleanly isolated via explicit interfaces.

---

## 🏗️ Layered Architecture

Turnix is organized into four main vertical layers:

```
+-----------------------------------------------------------+
|                        USER SPACE                         |
|   init daemon | interactive shell | fault-tester | apps   |
+-----------------------------------------------------------+
                             |
                   System Call Boundary
                             |
+-----------------------------------------------------------+
|                       KERNEL CORE                         |
|   Scheduler   |   VFS (Mounts)   |  VMM (Demand Paging)   |
|   Signals     |   Pipes & IPC    |  Capabilities / LSM    |
+-----------------------------------------------------------+
                             |
+-----------------------------------------------------------+
|                     DRIVER FRAMEWORK                      |
|   PCIe ECAM   |   ACPI AML Evaluator   |  DRM Framebuffer |
|   VirtIO-Net  |   NVMe Controller      |  XHCI USB Host   |
+-----------------------------------------------------------+
                             |
+-----------------------------------------------------------+
|                     FIRMWARE & BOOT                       |
|          UEFI Loader  <--->  EDK2 / OVMF Firmware         |
+-----------------------------------------------------------+
```

---

## 🛡️ Core Architectural Components

### 1. Firmware and Boot (UEFI)
*   **Thin Loader**: Located in `boot/uefi-loader`, it executes under UEFI firmware, reads the freestanding kernel ELF from disk, sets up basic identity paging, exits UEFI boot services, and transfers control to the kernel with a structured `BootInfo` handoff.

### 2. Memory Management Subsystem (VMM)
*   **Virtual Memory Areas (VMAs)**: Tracks virtual address space allocations (`VmaSet`) via sorted maps to prevent region overlaps.
*   **Demand Paging**: Zero-fills user pages on access, intercepts page faults, and maps physical frames dynamically.
*   **ASLR & KASLR**: Randomizes load bases for user applications (`exec`) and slides the kernel entry point dynamically (`KASLR`) based on `RDRAND` boot seeds.
*   **W^X Hardening**: A boot self-check walk ensures that no page in the page tables holds both Writable and Executable permissions.
*   **Swap & Reclaim**: Clock-based LRU evicts cold anonymous frames to a swap partition when free memory runs low.
*   **OOM Killer**: Calculates process scores based on resident set sizes (RSS) and priority to safely reclaim memory when resources are exhausted.

### 3. POSIX System Services
*   **Process Table**: PCB tracking process state, signal masks, and file descriptors.
*   **Lifecycle**: Safe kernel wrappers for `fork`, `exec` segment loading, and parent `wait`/`waitpid` reaping.
*   **IPC**: High-throughput ring-buffered pipes (triggering `SIGPIPE` on write to closed readers) and Unix Domain sockets (`AF_UNIX`).
*   **VFS**: Supports filesystem mount tables resolved by mount-point length, translating requests to `tmpfs` and `ext4` filesystem drivers.

### 4. Pluggable Security Layer
*   **POSIX Capabilities**: standard 64-bit capability sets (`permitted`, `effective`, `inheritable`, `bounding`, `ambient`) to restrict privileged operations.
*   **Namespaces**: Mount, Network, PID (local init remapping), and User (UID/GID translation) namespaces.
*   **Seccomp-BPF**: System call filter inheritance using classic BPF evaluation.
*   **LSM Hook Framework**: Pluggable hooks with a default Unix DAC implementation.
*   **IMA/EVM**: Integrity Measurement Architecture log ring buffer and EVM checksum verifying file metadata.
*   **Stack Canaries**: Stack corruption checks placed at the base of task stacks.

### 5. Unified Driver Framework
*   **Registration**: A unified `DeviceRegistry` holds device descriptors. Drivers implement the `DeviceDriver` trait with `probe`, `initialize`, `suspend`, and `resume` entry points.
*   **ACPI/PCIe**: Evaluates ACPI MCFG tables to map ECAM addresses, enumerate all PCIe devices, and parse AML DSDT namespaces for power management.
*   **I/O Drivers**: Custom drivers for VirtIO-Net negotiation, NVMe command queues (PRP-based data transfers), and USB XHCI hosts (handling USB Keyboard input events).

---

## 📝 Design Invariants

*   **Minimizing Unsafe**: Unsafe Rust is constrained to low-level hardware registers, page tables, and context switching. Every `unsafe` block must be documented with a `// SAFETY:` invariant check.
*   **Concurrency Safety**: Synchronization is enforced using lock abstractions (`spin::Mutex`). Lock ordering rules are documented to prevent deadlocks.
*   **Observability**: System progress is exposed via serial logging and structured log logs.
