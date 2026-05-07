# Requirements Document

## Introduction

Turnix OS is a Rust-first x86_64 hobby/research operating system currently at Phase 6 of 8. It has a working UEFI boot chain, preemptive Round-Robin scheduler, Ring 3/Ring 0 separation, SYSCALL/SYSRET, ELF loader, a flat in-memory VFS, serial output, PS/2 keyboard, and a PSF font framebuffer renderer. SMP APs start but halt; the network stack is a stub with hardcoded fake MAC/IP; the security model is a 5-flag custom struct rather than POSIX capabilities.

The production-readiness evaluation scores are: Hardware Enablement 7/100, Package Management Parity 0/100, Security Framework Compliance 4/100, System Services Readiness 17/100 — a total of 28/400 against a migration-kit threshold of 320/400.

This document specifies requirements for the eight subsystem phases needed to close that gap and bring Turnix to desktop production-readiness. The philosophy to preserve throughout is: Rust-first userspace, Linux-like workability without binary compatibility, secure by default with least privilege, rollback-friendly, and a clean documented codebase with ADRs for every major decision.

**Decisions recorded from project owner (May 2026):**
- **AArch64**: Arch abstraction boundary and stubs are in scope; actual AArch64 boot is deferred to a future spec. The boundary must be clean enough that porting is a matter of filling in stubs, not restructuring the kernel.
- **Filesystem**: ext2 (read-only) and ext4 (read-write) are the required backends. Btrfs, NTFS, and other filesystems are explicitly deferred but the VFS backend trait must be designed to accommodate them without changes to the VFS core.
- **Network stack**: smoltcp-based TCP/IP stack is in scope. Basic Wi-Fi support (via virtio-wifi in QEMU, real 802.11 driver deferred) is required so that a browser and network-dependent applications can function.
- **Package format**: The native `.tpkg` format is Rust-centric (Cargo-based builds) but the package manifest and install pipeline must support pre-built ELF binaries from non-Rust build systems (C, C++, Python wheels, etc.) so that browsers, music players, and other existing open-source software can be packaged and installed.
- **CI runner**: GitHub Actions (free tier, open-source) using QEMU software emulation. All benchmark targets are calibrated for a GitHub-hosted `ubuntu-latest` runner with QEMU 8.x. No self-hosted hardware is assumed.

---

## Glossary

- **Turnix**: The Turnix OS kernel and its associated userland, the system under specification.
- **Kernel**: The Ring 0 monolithic Rust kernel binary.
- **Driver_Framework**: The unified Rust trait-based device model that replaces ad-hoc driver registration.
- **Device_Registry**: The runtime table of probed and initialised hardware devices, analogous to Linux sysfs/kobject.
- **PCIe_Enumerator**: The kernel subsystem that walks the PCIe configuration space and populates the Device_Registry.
- **XHCI_Driver**: The USB 3.x host-controller driver.
- **NVMe_Driver**: The NVMe block-device driver.
- **GPU_Framebuffer**: The kernel-side linear framebuffer abstraction for GPU output (no 3D acceleration in scope).
- **VirtIO_Net**: The virtio-net paravirtualised network driver used for QEMU bring-up.
- **ACPI_Interpreter**: The kernel subsystem that evaluates DSDT/SSDT AML bytecode.
- **MM**: The memory-management subsystem.
- **Page_Cache**: The kernel in-memory cache of file-backed pages.
- **Swap_Manager**: The subsystem that evicts cold pages to a swap device and restores them on fault.
- **OOM_Killer**: The kernel policy that selects and terminates a process when physical memory is exhausted.
- **ASLR**: Address Space Layout Randomisation — randomises the base addresses of process segments.
- **KASLR**: Kernel Address Space Layout Randomisation — randomises the kernel load address at boot.
- **WX_Enforcer**: The page-table enforcement that ensures no page is simultaneously Writable and Executable.
- **Init_Daemon**: The PID-1 process that bootstraps userland and supervises services.
- **Service_Manager**: The userland daemon that starts, stops, and monitors system services.
- **VFS**: The Virtual File System layer that presents a unified namespace over multiple filesystem backends.
- **Mount_Manager**: The VFS subsystem that attaches and detaches filesystem backends at mount points.
- **Pipe**: A unidirectional kernel-buffered byte channel between two file descriptors.
- **Unix_Socket**: A bidirectional IPC endpoint addressed by a filesystem path.
- **Signal_Dispatcher**: The kernel subsystem that delivers POSIX signals to processes.
- **Capability_Set**: The per-process 64-bit POSIX capability bitmask (effective, permitted, inheritable, bounding, ambient).
- **Namespace**: A kernel isolation boundary for PID, mount, network, or user resources.
- **Seccomp_Filter**: A BPF program attached to a process that restricts the set of allowed syscalls.
- **LSM**: Linux Security Module — the hook-based MAC framework adapted for Turnix.
- **Stack_Canary**: A random sentinel value placed on the stack to detect buffer overflows.
- **IMA**: Integrity Measurement Architecture — records cryptographic hashes of executed files.
- **TPM**: Trusted Platform Module — hardware root of trust used by IMA/EVM.
- **Package_Manager**: The Turnix native tool for installing, updating, and removing software packages.
- **TUF**: The Update Framework — a specification for secure, signed software repository metadata.
- **Dependency_Solver**: The SAT-based algorithm that resolves package dependency constraints.
- **Snapshot_Manager**: The filesystem-level mechanism for atomic install and rollback via copy-on-write snapshots.
- **IPC_Broker**: The async message-passing daemon that replaces D-Bus for inter-process communication.
- **Log_Daemon**: The structured logging daemon that collects, seals, and rotates kernel and userland log streams.
- **Network_Manager**: The userland daemon that configures network interfaces and manages connections.
- **Device_Manager**: The userland daemon that handles hotplug events and loads drivers.
- **Compositor**: The Wayland compositor that manages surfaces, input routing, and display output.
- **DRM_KMS**: Direct Rendering Manager / Kernel Mode Setting — the kernel display pipeline.
- **GBM**: Generic Buffer Manager — the kernel-side buffer allocation interface for the GPU.
- **Input_Manager**: The userland input-event normalisation layer (libinput equivalent).
- **Display_Manager**: The greeter daemon that authenticates users and launches desktop sessions.
- **Desktop_Shell**: The minimal graphical shell providing a taskbar, launcher, and window management.
- **CI_Gate**: The automated quality gate that must pass before any commit merges to the integration branch.
- **QEMU**: The machine emulator used as the primary test platform.
- **CIS_Benchmark**: Center for Internet Security benchmark used as the reference for security regression tests.
- **ADR**: Architecture Decision Record — a short document recording a significant design choice.

---

## Phase 1 — Kernel Configuration and Driver Framework

### Requirement 1: Unified Driver Framework

**User Story:** As a kernel developer, I want a unified Rust trait-based driver framework, so that hardware drivers can be registered, probed, and managed consistently without ad-hoc global state.

#### Acceptance Criteria

1. THE Driver_Framework SHALL define a `DeviceDriver` trait with at minimum `probe`, `initialize`, `suspend`, and `resume` methods, each returning a typed `Result`.
2. THE Driver_Framework SHALL maintain a Device_Registry that maps device identifiers to initialised driver instances.
3. WHEN a new driver crate is compiled into the kernel, THE Driver_Framework SHALL register it in the Device_Registry without requiring changes to any other driver crate.
4. WHEN `probe` returns an error for a device, THE Driver_Framework SHALL log the error with the device identifier and continue probing remaining devices.
5. THE Driver_Framework SHALL enforce that all driver state is owned by the driver instance and not stored in kernel-global mutable statics.
6. THE Driver_Framework SHALL provide a typestate pattern for driver lifecycle transitions (`Unprobed` → `Probed` → `Initialised` → `Suspended`) enforced at compile time.

---

### Requirement 2: PCIe Enumeration

**User Story:** As a kernel developer, I want full PCIe configuration-space enumeration, so that all attached PCI devices are discovered and registered at boot.

#### Acceptance Criteria

1. WHEN the kernel boots, THE PCIe_Enumerator SHALL walk all PCIe buses, devices, and functions reachable via ECAM or legacy I/O-port configuration space.
2. THE PCIe_Enumerator SHALL populate the Device_Registry with an entry for each discovered device containing vendor ID, device ID, class code, subclass, and BAR addresses.
3. WHEN a PCIe device has a Base Address Register of type 64-bit memory, THE PCIe_Enumerator SHALL map the BAR into the kernel virtual address space before registering the device.
4. IF a PCIe configuration-space read returns `0xFFFFFFFF`, THEN THE PCIe_Enumerator SHALL treat the slot as absent and skip it without panicking.
5. THE PCIe_Enumerator SHALL complete enumeration within 500 ms of kernel entry on a QEMU machine with 32 or fewer PCIe devices.

---

### Requirement 3: USB XHCI Driver

**User Story:** As a user, I want USB 3.x host-controller support, so that USB keyboards, mice, and storage devices work on real hardware.

#### Acceptance Criteria

1. WHEN an XHCI controller is present in the Device_Registry, THE XHCI_Driver SHALL initialise the controller and enumerate attached USB devices.
2. THE XHCI_Driver SHALL support USB 2.0 and USB 3.x devices on the same root hub.
3. WHEN a USB HID device is attached, THE XHCI_Driver SHALL deliver input events to the Input_Manager within 10 ms of the USB interrupt.
4. WHEN a USB mass-storage device is attached, THE XHCI_Driver SHALL expose it as a block device in the Device_Registry.
5. IF the XHCI controller fails to reset within 1 second, THEN THE XHCI_Driver SHALL log the failure and mark the controller as unavailable without halting the kernel.

---

### Requirement 4: NVMe Block Driver

**User Story:** As a user, I want NVMe SSD support, so that the OS can boot from and store data on modern NVMe storage.

#### Acceptance Criteria

1. WHEN an NVMe controller is present in the Device_Registry, THE NVMe_Driver SHALL initialise admin and I/O submission/completion queues.
2. THE NVMe_Driver SHALL support read and write operations on NVMe namespaces with a block size of 512 bytes or 4096 bytes.
3. WHEN an I/O command completes, THE NVMe_Driver SHALL signal the waiting kernel thread within 1 ms on QEMU with virtio-blk emulation.
4. THE NVMe_Driver SHALL expose each NVMe namespace as a block device in the Device_Registry with its capacity in bytes.
5. IF an NVMe command times out after 30 seconds, THEN THE NVMe_Driver SHALL abort the command, log the timeout, and return an I/O error to the caller.

---

### Requirement 5: GPU Linear Framebuffer Driver

**User Story:** As a developer, I want a kernel-side linear framebuffer abstraction, so that the Wayland compositor can render pixels without requiring 3D GPU acceleration.

#### Acceptance Criteria

1. WHEN a GPU device is present in the Device_Registry, THE GPU_Framebuffer SHALL negotiate a linear framebuffer mode via DRM_KMS.
2. THE GPU_Framebuffer SHALL expose the framebuffer as a memory-mapped region accessible to the Compositor process via a dedicated syscall.
3. THE GPU_Framebuffer SHALL support at minimum 1920×1080 resolution at 32 bits per pixel.
4. WHEN the display resolution changes, THE GPU_Framebuffer SHALL notify the Compositor via an event and remap the framebuffer region.
5. THE GPU_Framebuffer SHALL enforce that only the Compositor process may map the framebuffer region; all other mapping attempts SHALL return `EPERM`.

---

### Requirement 6: VirtIO-Net Network Driver

**User Story:** As a developer, I want a virtio-net driver for QEMU, so that the network stack can be validated in the emulated environment before real NIC drivers are written.

#### Acceptance Criteria

1. WHEN a virtio-net device is present in the Device_Registry, THE VirtIO_Net driver SHALL initialise the virtio queues and negotiate the `VIRTIO_NET_F_MAC` feature to obtain the device MAC address.
2. THE VirtIO_Net driver SHALL replace the current hardcoded fake MAC/IP with the MAC address negotiated from the virtio device.
3. WHEN a network packet is received, THE VirtIO_Net driver SHALL deliver it to the network stack within 5 ms on QEMU.
4. THE VirtIO_Net driver SHALL support transmit and receive ring sizes of at least 256 descriptors each.
5. IF the virtio device resets unexpectedly, THEN THE VirtIO_Net driver SHALL attempt re-initialisation once and log the outcome.

---

### Requirement 7: Full ACPI Evaluation

**User Story:** As a kernel developer, I want full ACPI DSDT/SSDT AML evaluation, so that hardware power management, device enumeration, and interrupt routing work correctly on real machines.

#### Acceptance Criteria

1. WHEN the kernel boots, THE ACPI_Interpreter SHALL locate and parse the RSDP, XSDT, DSDT, and all SSDTs from the ACPI tables provided by UEFI.
2. THE ACPI_Interpreter SHALL evaluate the `\_SB` namespace to discover platform devices not enumerable via PCIe.
3. THE ACPI_Interpreter SHALL evaluate `_PRT` tables to resolve PCIe interrupt routing and program the IOAPIC accordingly.
4. THE ACPI_Interpreter SHALL support ACPI power states S0 (working) and S5 (soft off) at minimum.
5. WHEN the system receives an ACPI power-button event, THE ACPI_Interpreter SHALL deliver a shutdown signal to the Init_Daemon within 500 ms.
6. IF an AML method execution exceeds 100 ms, THEN THE ACPI_Interpreter SHALL abort the method, log a warning with the method path, and continue boot.

---

### Requirement 8: AArch64 Port Groundwork

**User Story:** As a kernel developer, I want an AArch64 architecture abstraction layer, so that future porting work has a clean boundary between architecture-specific and architecture-independent kernel code.

#### Acceptance Criteria

1. THE Kernel SHALL define an `arch` module boundary that isolates all x86_64-specific code (GDT, IDT, LAPIC, SYSCALL/SYSRET, MSRs) behind architecture-specific trait implementations.
2. THE Kernel SHALL compile without errors for the `aarch64-unknown-none` target when all architecture-specific modules are excluded via Cargo feature flags.
3. THE Kernel SHALL provide stub implementations of all architecture traits for AArch64 that return `Err(NotImplemented)` at runtime.
4. WHEN a new ADR is written for any architecture-specific decision, THE ADR SHALL document the equivalent AArch64 mechanism as a future consideration.


---

## Phase 2 — Memory Management Maturity

### Requirement 9: Demand Paging

**User Story:** As a kernel developer, I want demand paging, so that processes only consume physical memory for pages they actually access, enabling larger address spaces than physical RAM.

#### Acceptance Criteria

1. WHEN a process accesses a virtual address that is mapped but not yet backed by a physical frame, THE MM SHALL handle the page fault by allocating a frame and mapping it without terminating the process.
2. THE MM SHALL support anonymous demand-paged mappings created via the `mmap` syscall with `MAP_ANONYMOUS`.
3. WHEN a demand-paged region is first accessed, THE MM SHALL zero-fill the allocated frame before mapping it into the process address space.
4. THE MM SHALL track virtual memory areas (VMAs) per process with start address, end address, protection flags, and backing type.
5. IF a page fault occurs at an address not covered by any VMA, THEN THE MM SHALL deliver `SIGSEGV` to the faulting process.

---

### Requirement 10: Page Cache

**User Story:** As a kernel developer, I want a page cache, so that repeated reads of the same file data are served from memory rather than re-reading from storage.

#### Acceptance Criteria

1. WHEN a file read syscall is issued, THE Page_Cache SHALL check whether the requested pages are already cached and return cached data without issuing a block I/O request.
2. WHEN a file page is not in the Page_Cache, THE Page_Cache SHALL issue a block I/O read, populate the cache entry, and return the data to the caller.
3. THE Page_Cache SHALL evict the least-recently-used pages when free physical memory falls below a configurable low-watermark threshold.
4. WHEN a cached page is modified by a write syscall, THE Page_Cache SHALL mark the page dirty and schedule it for writeback within 30 seconds.
5. THE Page_Cache SHALL share pages between multiple processes that have mapped the same file, using a single physical frame per file page.

---

### Requirement 11: Swap

**User Story:** As a kernel developer, I want swap support, so that the system can continue operating when physical memory is under pressure by evicting cold pages to a swap device.

#### Acceptance Criteria

1. THE Swap_Manager SHALL support a swap partition identified by a dedicated partition type GUID in the GPT.
2. WHEN free physical memory falls below the low-watermark threshold, THE Swap_Manager SHALL select cold anonymous pages for eviction using a clock or LRU approximation algorithm.
3. WHEN a page is evicted to swap, THE Swap_Manager SHALL write the page to the swap device and update the page table entry to record the swap slot.
4. WHEN a process accesses a swapped-out page, THE MM SHALL handle the page fault by reading the page from the swap device, allocating a frame, and restoring the mapping.
5. THE Swap_Manager SHALL not evict pages that are locked (e.g., DMA buffers or pages pinned by the kernel).

---

### Requirement 12: OOM Handling

**User Story:** As a kernel developer, I want an OOM killer, so that the system recovers gracefully when physical memory and swap are both exhausted rather than panicking.

#### Acceptance Criteria

1. WHEN both physical memory and swap are exhausted and a new allocation is requested, THE OOM_Killer SHALL select a victim process using an OOM score based on resident set size and process priority.
2. WHEN a victim is selected, THE OOM_Killer SHALL deliver `SIGKILL` to the victim process and log the victim PID, name, and OOM score.
3. THE OOM_Killer SHALL not select PID 1 (Init_Daemon) or kernel threads as OOM victims.
4. WHEN the OOM_Killer has delivered `SIGKILL`, THE MM SHALL retry the failed allocation after the victim's memory is reclaimed.
5. IF memory is not reclaimed within 5 seconds of OOM kill, THEN THE OOM_Killer SHALL attempt to kill the next highest-scoring process.

---

### Requirement 13: ASLR

**User Story:** As a security engineer, I want address space layout randomisation, so that exploit techniques relying on known memory addresses are defeated.

#### Acceptance Criteria

1. WHEN a new process is created via `exec`, THE ASLR subsystem SHALL randomise the base address of the executable load region, the stack, and all `mmap` regions using at least 28 bits of entropy on x86_64.
2. THE ASLR subsystem SHALL use a cryptographically seeded PRNG seeded from hardware entropy (RDRAND or TPM) at boot.
3. WHEN a process is forked, THE ASLR subsystem SHALL assign new random base addresses to the child's stack and heap regions.
4. THE ASLR subsystem SHALL preserve the relative layout of ELF segments within a load region while randomising the region base.
5. WHERE a binary is compiled as a Position-Independent Executable, THE ASLR subsystem SHALL randomise its load address; WHERE a binary is not position-independent, THE ASLR subsystem SHALL log a warning and load it at its preferred address.

---

### Requirement 14: W^X Enforcement

**User Story:** As a security engineer, I want strict W^X page-table enforcement, so that no memory region is simultaneously writable and executable, preventing code-injection attacks.

#### Acceptance Criteria

1. THE WX_Enforcer SHALL ensure that no page table entry in any process address space has both the `WRITABLE` and executable (not `NO_EXECUTE`) flags set simultaneously.
2. WHEN a process attempts to create a mapping with both write and execute permissions via `mmap`, THE WX_Enforcer SHALL return `EACCES` and log the attempt.
3. THE WX_Enforcer SHALL enforce W^X for kernel memory regions as well as user memory regions.
4. WHEN the kernel loads an ELF segment marked both writable and executable, THE WX_Enforcer SHALL map it as writable-only during load, then revoke write permission before transferring control to the entry point.
5. THE WX_Enforcer SHALL run a self-check at boot that verifies no kernel page violates W^X and logs the result.

---

### Requirement 15: KASLR

**User Story:** As a security engineer, I want kernel address space layout randomisation, so that kernel addresses are not predictable and kernel exploits requiring known addresses are harder to execute.

#### Acceptance Criteria

1. WHEN the UEFI loader transfers control to the kernel, THE Kernel SHALL have been loaded at a base address randomised by at least 9 bits of entropy within the higher-half kernel region.
2. THE Kernel SHALL derive the KASLR offset from RDRAND or from the UEFI-provided random seed before any kernel data structures are initialised.
3. THE Kernel SHALL apply the KASLR offset to all internal symbol references via position-independent relocation at entry.
4. WHEN KASLR is active, THE Kernel SHALL not expose the kernel base address to unprivileged userland processes via any syscall or `/proc`-equivalent interface.


---

## Phase 3 — POSIX-Compatible System Services

### Requirement 16: Init Daemon (PID 1)

**User Story:** As a system integrator, I want a robust PID-1 init daemon, so that userland bootstraps reliably and all orphaned processes are reaped correctly.

#### Acceptance Criteria

1. THE Init_Daemon SHALL be the first userland process spawned by the kernel with PID 1.
2. THE Init_Daemon SHALL read a service manifest from a well-known path (e.g., `/etc/turnix/services/`) and start each declared service in dependency order.
3. WHEN a child process of PID 1 exits, THE Init_Daemon SHALL call `wait` to reap it within 1 second and log the exit code.
4. WHEN any process becomes an orphan (its parent exits), THE Kernel SHALL reparent it to PID 1.
5. WHEN the Init_Daemon receives `SIGTERM` or an ACPI power-button event, THE Init_Daemon SHALL stop all services in reverse dependency order and then call the `shutdown` syscall.
6. IF a service fails to start within its configured timeout, THE Init_Daemon SHALL log the failure and continue starting remaining services.

---

### Requirement 17: Full Process Lifecycle (fork/exec/wait)

**User Story:** As a developer, I want fully working fork, exec, and wait primitives, so that standard Unix process management patterns work correctly.

#### Acceptance Criteria

1. WHEN `fork` is called, THE Kernel SHALL create a child process with a copy-on-write clone of the parent's address space, file descriptor table, and signal mask, returning 0 to the child and the child PID to the parent.
2. WHEN `exec` is called with a valid ELF path, THE Kernel SHALL replace the calling process's address space with the new ELF image, preserving the PID and open file descriptors marked `O_CLOEXEC` as closed.
3. WHEN `wait` or `waitpid` is called, THE Kernel SHALL block the caller until the specified child exits and return the child's exit status.
4. WHEN a process calls `exit`, THE Kernel SHALL release its address space, close all file descriptors, and transition the process to the `Zombie` state until its parent calls `wait`.
5. IF `exec` fails because the ELF file is not found, THEN THE Kernel SHALL return `ENOENT` and leave the calling process unchanged.
6. THE Kernel SHALL support at least 1024 concurrent processes.

---

### Requirement 18: Pipes and Unix Domain Sockets

**User Story:** As a developer, I want pipes and Unix domain sockets, so that processes can communicate using standard IPC mechanisms.

#### Acceptance Criteria

1. WHEN `pipe` is called, THE Kernel SHALL create a pair of file descriptors where writes to the write end are readable from the read end, with a kernel buffer of at least 65536 bytes.
2. WHEN the write end of a Pipe is closed and all buffered data has been read, THE Kernel SHALL return 0 (EOF) to subsequent reads on the read end.
3. WHEN a write to a Pipe would exceed the buffer capacity, THE Kernel SHALL block the writing process until space is available.
4. WHEN `socket(AF_UNIX, SOCK_STREAM, 0)` is called, THE Kernel SHALL create a Unix_Socket endpoint.
5. WHEN a Unix_Socket server calls `bind` with a filesystem path, THE Kernel SHALL create a socket file at that path in the VFS.
6. WHEN a Unix_Socket client calls `connect` to a bound path, THE Kernel SHALL establish a bidirectional byte stream between client and server.
7. IF a process writes to a Pipe whose read end has been closed, THEN THE Kernel SHALL deliver `SIGPIPE` to the writing process.

---

### Requirement 19: Signal Handling

**User Story:** As a developer, I want POSIX signal handling, so that processes can respond to asynchronous events and inter-process notifications.

#### Acceptance Criteria

1. THE Signal_Dispatcher SHALL support the standard POSIX signals: `SIGHUP`, `SIGINT`, `SIGQUIT`, `SIGILL`, `SIGABRT`, `SIGFPE`, `SIGKILL`, `SIGSEGV`, `SIGPIPE`, `SIGALRM`, `SIGTERM`, `SIGCHLD`, `SIGCONT`, `SIGSTOP`, `SIGTSTP`, `SIGTTIN`, `SIGTTOU`, `SIGUSR1`, `SIGUSR2`.
2. WHEN a process calls `sigaction` with a valid signal number and handler, THE Signal_Dispatcher SHALL register the handler and deliver the signal to that handler on the next return to user mode.
3. WHEN `SIGKILL` or `SIGSTOP` is sent to a process, THE Signal_Dispatcher SHALL not allow the process to catch or ignore those signals.
4. WHEN a signal is delivered, THE Signal_Dispatcher SHALL save the process's current register state on the user stack and transfer control to the signal handler.
5. WHEN the signal handler returns via `sigreturn`, THE Signal_Dispatcher SHALL restore the saved register state and resume normal execution.
6. THE Signal_Dispatcher SHALL support signal masks set via `sigprocmask` to block delivery of specified signals.

---

### Requirement 20: POSIX File Descriptors (stdin/stdout/stderr)

**User Story:** As a developer, I want standard POSIX file descriptors, so that programs can use stdin, stdout, and stderr without custom I/O APIs.

#### Acceptance Criteria

1. WHEN the Init_Daemon starts, THE Kernel SHALL open `/dev/tty` as file descriptor 0 (stdin), 1 (stdout), and 2 (stderr) for PID 1.
2. WHEN a process is forked, THE Kernel SHALL inherit file descriptors 0, 1, and 2 from the parent unless they are marked `O_CLOEXEC`.
3. WHEN a process writes to file descriptor 1 or 2, THE VFS SHALL route the write to the TTY device and display the output on the console.
4. WHEN a process reads from file descriptor 0, THE VFS SHALL block until a line of input is available from the TTY device.
5. THE Kernel SHALL support `dup` and `dup2` syscalls to duplicate file descriptors.
6. THE Kernel SHALL support at least 1024 open file descriptors per process (replacing the current 16-FD limit).

---

### Requirement 21: Real VFS with Mount Points and Multiple Filesystem Backends

**User Story:** As a developer, I want a real VFS with mount points and pluggable filesystem backends, so that the OS can mount different filesystems at different paths and present a unified namespace.

#### Acceptance Criteria

1. THE VFS SHALL maintain a mount table mapping mount points (absolute paths) to filesystem backend instances.
2. WHEN `mount` is called with a device path, filesystem type, and mount point, THE Mount_Manager SHALL attach the filesystem backend to the VFS namespace at the specified path.
3. WHEN `umount` is called for a mount point with no open file descriptors, THE Mount_Manager SHALL detach the filesystem backend and free its resources.
4. THE VFS SHALL support at minimum three filesystem backends: the existing in-memory tmpfs, ext2 read-only, and a read-write ext2 or ext4 implementation.
5. WHEN a path lookup crosses a mount point boundary, THE VFS SHALL transparently delegate to the mounted filesystem backend.
6. THE VFS SHALL support the following operations on all backends: `open`, `close`, `read`, `write`, `seek`, `stat`, `readdir`, `mkdir`, `unlink`, `rename`.
7. THE VFS SHALL enforce per-file permission bits (owner read/write/execute, group, other) on all operations.


---

## Phase 4 — Security Framework

### Requirement 22: POSIX Capabilities (Full 64-bit Set)

**User Story:** As a security engineer, I want the full POSIX 64-bit capability set, so that privilege can be granted at fine granularity rather than as an all-or-nothing root flag.

#### Acceptance Criteria

1. THE Kernel SHALL replace the current 5-flag `Capabilities` struct with a full POSIX capability model comprising five 64-bit sets per process: effective, permitted, inheritable, bounding, and ambient.
2. THE Kernel SHALL enforce capability checks at every privileged operation (e.g., `CAP_NET_ADMIN` for network configuration, `CAP_SYS_ADMIN` for mount, `CAP_KILL` for sending signals to other users' processes).
3. WHEN a process calls `exec`, THE Kernel SHALL compute the new capability sets according to the POSIX capability transformation rules (permitted = (inheritable & file_inheritable) | (file_permitted & bounding)).
4. WHEN a process drops a capability from its permitted set, THE Kernel SHALL prevent that capability from being re-acquired without a new `exec` of a file with the capability in its file permitted set.
5. THE Kernel SHALL expose capability sets to userland via `capget` and `capset` syscalls.
6. THE Kernel SHALL support file capabilities stored as extended attributes on filesystem inodes.

---

### Requirement 23: Namespace Isolation

**User Story:** As a security engineer, I want PID, mount, network, and user namespace isolation, so that containerised processes cannot observe or interfere with resources outside their namespace.

#### Acceptance Criteria

1. THE Kernel SHALL support PID namespaces such that a process in a child PID namespace sees its own PID as 1 and cannot observe PIDs in the parent namespace.
2. THE Kernel SHALL support mount namespaces such that `mount` and `umount` operations in a child namespace do not affect the parent namespace's mount table.
3. THE Kernel SHALL support network namespaces such that each namespace has its own set of network interfaces, routing tables, and socket tables.
4. THE Kernel SHALL support user namespaces such that a process can map unprivileged UIDs/GIDs in the parent namespace to privileged UIDs/GIDs within the child namespace.
5. WHEN `clone` is called with `CLONE_NEWPID | CLONE_NEWNS | CLONE_NEWNET | CLONE_NEWUSER`, THE Kernel SHALL create a new process in all four new namespaces simultaneously.
6. THE Kernel SHALL enforce that a process cannot escape its namespace by following symlinks or mount points that cross namespace boundaries without explicit permission.

---

### Requirement 24: Seccomp-BPF Syscall Filtering

**User Story:** As a security engineer, I want seccomp-BPF syscall filtering, so that sandboxed processes can be restricted to only the syscalls they need.

#### Acceptance Criteria

1. THE Kernel SHALL implement `seccomp(SECCOMP_SET_MODE_FILTER, ...)` to attach a BPF program to the calling process that is evaluated on every syscall entry.
2. WHEN a Seccomp_Filter returns `SECCOMP_RET_KILL_PROCESS`, THE Kernel SHALL immediately terminate the process with `SIGSYS` without executing the syscall.
3. WHEN a Seccomp_Filter returns `SECCOMP_RET_ERRNO(e)`, THE Kernel SHALL return the specified errno to the caller without executing the syscall.
4. WHEN a Seccomp_Filter returns `SECCOMP_RET_ALLOW`, THE Kernel SHALL execute the syscall normally.
5. THE Kernel SHALL enforce that once a seccomp filter is installed, the process cannot remove or replace it with a less restrictive filter.
6. WHEN a process forks, THE Kernel SHALL inherit the parent's seccomp filter in the child.

---

### Requirement 25: MAC Framework (LSM-Style Hooks)

**User Story:** As a security engineer, I want a Mandatory Access Control framework with LSM-style hooks, so that a security policy can be enforced independently of DAC permissions.

#### Acceptance Criteria

1. THE Kernel SHALL define LSM hook points at all security-sensitive operations: file open/read/write/exec, process creation, IPC send/receive, network connect/bind, and capability checks.
2. THE LSM SHALL support loading exactly one active MAC policy module at boot time, selected via a kernel command-line parameter.
3. WHEN an LSM hook is triggered and the active MAC policy denies the operation, THE Kernel SHALL return `EACCES` to the caller without performing the operation.
4. THE Kernel SHALL ship a default `permissive` MAC policy that logs all denials but allows all operations, suitable for policy development.
5. THE Kernel SHALL ship a `strict` MAC policy that enforces type-enforcement rules loaded from a policy file at `/etc/turnix/mac/policy.bin`.
6. WHEN the MAC policy file is absent or corrupt, THE Kernel SHALL fall back to the `permissive` policy and log a warning.

---

### Requirement 26: Stack Canaries

**User Story:** As a security engineer, I want stack canaries in the kernel and userland, so that stack buffer overflows are detected before a return address is corrupted.

#### Acceptance Criteria

1. THE Kernel SHALL be compiled with stack canary protection enabled (`-Z stack-protector=strong` or equivalent Rust/LLVM flag).
2. WHEN a stack canary value is corrupted at function return, THE Kernel SHALL trigger a kernel panic with a message identifying the corrupted function.
3. THE Kernel SHALL initialise the canary value from a hardware entropy source (RDRAND) at boot, not from a compile-time constant.
4. THE libturnix standard library SHALL provide stack canary support for userland binaries compiled against it.
5. WHEN a userland stack canary is corrupted, THE libturnix runtime SHALL deliver `SIGABRT` to the process before the corrupted return address is used.

---

### Requirement 27: IMA/EVM Groundwork

**User Story:** As a security engineer, I want Integrity Measurement Architecture groundwork, so that the system can record and verify the integrity of executed files.

#### Acceptance Criteria

1. WHEN a file is executed via `exec`, THE IMA subsystem SHALL compute the SHA-256 hash of the file's contents and record it in an in-memory measurement log.
2. THE IMA subsystem SHALL expose the measurement log to privileged processes via a read-only file at `/sys/kernel/security/ima/ascii_runtime_measurements`.
3. WHERE a TPM is present, THE IMA subsystem SHALL extend PCR 10 with each new measurement.
4. THE IMA subsystem SHALL support an `enforce` mode in which `exec` is denied for any file whose hash does not match a pre-loaded policy.
5. THE IMA subsystem SHALL support a `log` mode (default) in which all measurements are recorded but no `exec` is denied.


---

### Requirement 6a: TCP/IP Network Stack (smoltcp)

**User Story:** As a developer, I want a working TCP/IP stack, so that applications like browsers and package managers can make network connections.

#### Acceptance Criteria

1. THE Network_Stack SHALL be implemented using the `smoltcp` crate (open-source, MIT/Apache-2.0 licensed) integrated with the VirtIO_Net driver.
2. THE Network_Stack SHALL support IPv4 TCP, UDP, ICMP, and ARP at minimum.
3. WHEN a userland process calls `socket(AF_INET, SOCK_STREAM, 0)`, THE Kernel SHALL create a TCP socket backed by the smoltcp stack.
4. THE Network_Stack SHALL support DNS resolution via a stub resolver that queries a configured DNS server over UDP.
5. WHEN a DHCP lease is obtained, THE Network_Stack SHALL configure the interface IP address, subnet mask, default gateway, and DNS server automatically.
6. THE Network_Stack SHALL deliver received TCP data to the waiting userland process within 10 ms of packet arrival on QEMU.

---

### Requirement 6b: Basic Wi-Fi Support

**User Story:** As a user, I want basic Wi-Fi connectivity, so that the OS can connect to wireless networks without requiring a wired Ethernet connection.

#### Acceptance Criteria

1. THE Kernel SHALL support a virtio-wifi (or mac80211_hwsim equivalent) driver for QEMU-based Wi-Fi testing.
2. THE Network_Manager SHALL include a Wi-Fi connection manager that can scan for SSIDs, associate with a WPA2-PSK network, and obtain a DHCP lease.
3. WHEN a Wi-Fi network profile is saved in `/etc/turnix/network/`, THE Network_Manager SHALL reconnect automatically on boot without user interaction.
4. THE Wi-Fi subsystem SHALL be designed with a hardware abstraction layer (cfg80211 equivalent) so that real 802.11 NIC drivers (Intel iwlwifi, Realtek rtw88) can be added as driver crates without changing the Wi-Fi manager.
5. IF Wi-Fi association fails after 3 attempts, THE Network_Manager SHALL log the failure and enter a retry loop with 30-second back-off.

---

## Phase 5 — Package Management

### Requirement 28: Native Turnix Package Format

**User Story:** As a developer, I want a native Turnix package format based on Cargo crates, so that software can be distributed and installed in a way that is idiomatic to the Rust ecosystem.

#### Acceptance Criteria

1. THE Package_Manager SHALL define a package format (`.tpkg`) that is a compressed archive containing: a compiled ELF binary or library, a manifest file (`turnix.toml`) with name, version, dependencies, and install paths, and optional pre/post-install scripts.
2. THE Package_Manager SHALL parse `turnix.toml` manifests using a grammar that is a strict subset of TOML, and SHALL return a descriptive error for any manifest that does not conform to the grammar.
3. THE Package_Manager SHALL support a round-trip property: FOR ALL valid `turnix.toml` manifests, parsing then serialising then parsing SHALL produce an equivalent manifest structure.
4. WHEN a `.tpkg` file is installed, THE Package_Manager SHALL verify the package signature against the repository's TUF root of trust before extracting any files.
5. IF a package signature verification fails, THEN THE Package_Manager SHALL abort the installation and return an error without modifying the filesystem.
6. THE Package_Manager SHALL support packaging pre-built ELF binaries produced by non-Rust build systems (C, C++, Go, Python, etc.) by accepting any ELF binary in the `.tpkg` archive regardless of the language it was compiled from.
7. THE Package_Manager SHALL support a `build-from-source` mode that invokes an arbitrary build command (e.g., `make`, `cmake`, `meson`) specified in `turnix.toml` under a `[build]` section, enabling open-source software like browsers and media players to be compiled and packaged.

---

### Requirement 29: Repository Metadata (TUF-Signed)

**User Story:** As a system administrator, I want TUF-signed repository metadata, so that package updates cannot be tampered with or rolled back by an attacker.

#### Acceptance Criteria

1. THE Package_Manager SHALL implement the TUF client specification to fetch and verify repository metadata (root, targets, snapshot, timestamp roles).
2. WHEN repository metadata is fetched, THE Package_Manager SHALL verify the signature chain from the root role to the targets role before trusting any package metadata.
3. WHEN a timestamp metadata file is older than 24 hours, THE Package_Manager SHALL treat the repository as potentially compromised and refuse to install packages from it until fresh metadata is fetched.
4. THE Package_Manager SHALL store verified metadata in a local cache and use it for offline operation when the repository is unreachable.
5. WHEN the TUF root key is rotated, THE Package_Manager SHALL follow the TUF key rotation protocol and update the local root of trust without requiring manual intervention.

---

### Requirement 30: Dependency Solver

**User Story:** As a developer, I want a dependency solver, so that installing a package automatically resolves and installs all required dependencies without conflicts.

#### Acceptance Criteria

1. THE Dependency_Solver SHALL accept a set of requested packages with version constraints and produce an installation plan that satisfies all constraints.
2. WHEN two packages in the dependency graph require incompatible versions of a shared dependency, THE Dependency_Solver SHALL return a conflict error listing the conflicting requirements.
3. THE Dependency_Solver SHALL complete dependency resolution for a graph of up to 500 packages within 5 seconds on a single CPU core.
4. THE Dependency_Solver SHALL prefer the newest compatible version of each dependency unless a stricter constraint is specified.
5. WHEN a circular dependency is detected, THE Dependency_Solver SHALL return an error identifying the cycle.

---

### Requirement 31: Atomic Install and Rollback via Filesystem Snapshots

**User Story:** As a system administrator, I want atomic package installation with rollback, so that a failed or interrupted install never leaves the system in a partially-modified state.

#### Acceptance Criteria

1. WHEN a package installation begins, THE Snapshot_Manager SHALL create a filesystem snapshot of the affected directories before any files are modified.
2. WHEN a package installation completes successfully, THE Snapshot_Manager SHALL commit the snapshot and make the new state the current state atomically.
3. WHEN a package installation fails or is interrupted, THE Snapshot_Manager SHALL automatically roll back to the pre-installation snapshot.
4. THE Package_Manager SHALL support an explicit `rollback` command that restores the system to the snapshot taken before the most recent install or upgrade.
5. THE Snapshot_Manager SHALL retain at minimum the last 3 snapshots to allow rollback across multiple operations.
6. WHEN a snapshot is created, THE Snapshot_Manager SHALL complete the snapshot operation within 2 seconds for a directory tree of up to 10,000 files.


---

## Phase 6 — System Services Layer

### Requirement 32: Init and Service Manager

**User Story:** As a system integrator, I want a systemd-inspired but Rust-native service manager, so that system services are started, supervised, and restarted reliably.

#### Acceptance Criteria

1. THE Service_Manager SHALL read service unit files from `/etc/turnix/services/` and `/usr/lib/turnix/services/` at startup.
2. THE Service_Manager SHALL start services in dependency order derived from `After=` and `Requires=` declarations in unit files.
3. WHEN a service process exits unexpectedly, THE Service_Manager SHALL restart it according to its `Restart=` policy (on-failure, always, or never) with a configurable back-off delay.
4. THE Service_Manager SHALL expose a control socket at `/run/turnix/service-manager.sock` accepting commands to start, stop, restart, and query the status of services.
5. WHEN a service fails to start within its `TimeoutStartSec` value, THE Service_Manager SHALL mark it as failed and log the timeout.
6. THE Service_Manager SHALL support service activation via socket (start a service when a connection arrives on its socket).

---

### Requirement 33: IPC Broker (D-Bus Equivalent)

**User Story:** As a developer, I want an async IPC broker, so that system services and applications can communicate via structured messages without implementing custom socket protocols.

#### Acceptance Criteria

1. THE IPC_Broker SHALL implement a message-passing protocol over Unix domain sockets supporting method calls, signals, and property access.
2. THE IPC_Broker SHALL enforce an access policy that restricts which processes may call which methods, based on the caller's UID and capability set.
3. WHEN a method call is dispatched, THE IPC_Broker SHALL deliver it to the target service and return the reply to the caller within 100 ms under normal load.
4. THE IPC_Broker SHALL support asynchronous signal delivery so that a subscriber receives a signal within 10 ms of the publisher emitting it.
5. THE IPC_Broker SHALL provide a Rust client library in `libturnix` for sending and receiving IPC messages.
6. WHEN the IPC_Broker process crashes, THE Service_Manager SHALL restart it within 1 second and reconnect all registered services automatically.

---

### Requirement 34: Logging Daemon (Structured, Sealed)

**User Story:** As a system administrator, I want a structured logging daemon with sealed log files, so that system logs are tamper-evident and queryable.

#### Acceptance Criteria

1. THE Log_Daemon SHALL collect log entries from the kernel ring buffer and from userland processes via a Unix socket at `/run/turnix/log.sock`.
2. THE Log_Daemon SHALL store log entries in a structured binary format that includes timestamp (nanosecond precision), severity, source process PID and name, and message.
3. THE Log_Daemon SHALL seal each completed log segment with an HMAC using a key derived from the TPM (or a software key if no TPM is present), making tampering detectable.
4. THE Log_Daemon SHALL rotate log files when a segment reaches 64 MB or 24 hours, whichever comes first.
5. THE Log_Daemon SHALL provide a query tool that filters log entries by time range, severity, and source process.
6. WHEN the Log_Daemon's storage partition is full, THE Log_Daemon SHALL delete the oldest sealed segment and log a warning before continuing.

---

### Requirement 35: Network Manager Daemon

**User Story:** As a user, I want a network manager daemon, so that network interfaces are configured automatically and connections can be managed without manual `ip` commands.

#### Acceptance Criteria

1. THE Network_Manager SHALL detect network interfaces registered in the Device_Registry and configure them via DHCP on first boot.
2. THE Network_Manager SHALL persist network configuration profiles in `/etc/turnix/network/` and apply them on subsequent boots.
3. WHEN a network interface link state changes (up/down), THE Network_Manager SHALL log the event and attempt to re-establish the connection if the profile specifies auto-connect.
4. THE Network_Manager SHALL expose a control interface via the IPC_Broker for querying interface status and modifying connection profiles.
5. THE Network_Manager SHALL support static IP configuration in addition to DHCP.

---

### Requirement 36: Device Manager Daemon

**User Story:** As a user, I want a device manager daemon, so that hotplugged hardware is automatically detected and the appropriate driver is loaded.

#### Acceptance Criteria

1. THE Device_Manager SHALL subscribe to hotplug events from the kernel's Device_Registry and receive notification within 100 ms of a device being attached or detached.
2. WHEN a hotplug event is received for a device with a known driver, THE Device_Manager SHALL request the kernel to load and initialise the driver for that device.
3. WHEN a USB storage device is attached, THE Device_Manager SHALL mount it at `/media/<device-label>` using the appropriate filesystem backend.
4. WHEN a USB storage device is detached, THE Device_Manager SHALL unmount it cleanly, flushing all pending writes before removing the mount point.
5. THE Device_Manager SHALL expose device information via the IPC_Broker so that desktop applications can query attached devices.


---

## Phase 7 — Wayland Graphical Stack

### Requirement 37: Wayland Compositor

**User Story:** As a user, I want a Wayland compositor, so that graphical applications can render windows and receive input events through a standard display protocol.

#### Acceptance Criteria

1. THE Compositor SHALL implement the Wayland core protocol (`wl_compositor`, `wl_surface`, `wl_shm`, `wl_seat`, `wl_output`) sufficient for a client to create and display a window.
2. THE Compositor SHALL implement the `xdg-shell` protocol extension to support toplevel windows with title bars, minimise, maximise, and close operations.
3. WHEN a client submits a buffer via `wl_surface.commit`, THE Compositor SHALL composite it onto the display within one frame period (16 ms at 60 Hz).
4. THE Compositor SHALL route keyboard and pointer events from the Input_Manager to the focused surface within 5 ms of the input event.
5. THE Compositor SHALL support at least 16 simultaneous client surfaces without frame drops at 60 Hz on hardware with a linear framebuffer.
6. WHEN a Wayland client process exits unexpectedly, THE Compositor SHALL remove its surfaces and continue compositing remaining clients without crashing.

---

### Requirement 38: DRM/KMS Display Pipeline

**User Story:** As a kernel developer, I want a DRM/KMS display pipeline, so that the compositor can control display mode setting and buffer flipping through a standard kernel interface.

#### Acceptance Criteria

1. THE DRM_KMS subsystem SHALL enumerate display connectors (HDMI, DisplayPort, eDP) and their supported modes via EDID.
2. THE DRM_KMS subsystem SHALL support atomic mode setting: applying a new display configuration (connector, CRTC, plane, mode) as a single atomic commit.
3. WHEN the Compositor calls the page-flip ioctl, THE DRM_KMS subsystem SHALL flip to the new buffer at the next vertical blank and deliver a vblank event to the Compositor.
4. THE DRM_KMS subsystem SHALL support at minimum one primary plane per CRTC for framebuffer display.
5. WHEN no display is connected, THE DRM_KMS subsystem SHALL report zero connectors and not panic.

---

### Requirement 39: GBM Buffer Allocation

**User Story:** As a compositor developer, I want a GBM buffer allocation interface, so that the compositor can allocate GPU-compatible buffers for rendering without GPU-vendor-specific APIs.

#### Acceptance Criteria

1. THE GBM subsystem SHALL provide an API to allocate a linear buffer with a specified width, height, and pixel format (at minimum `ARGB8888` and `XRGB8888`).
2. WHEN a buffer is allocated, THE GBM subsystem SHALL return a file descriptor that the Compositor can mmap for CPU rendering.
3. THE GBM subsystem SHALL support importing a buffer allocated by one process into another process via a DMA-BUF file descriptor.
4. WHEN a buffer is freed, THE GBM subsystem SHALL release the underlying physical memory and invalidate the associated file descriptor.

---

### Requirement 40: Input Manager (libinput Equivalent)

**User Story:** As a compositor developer, I want a normalised input event layer, so that keyboard, pointer, and touch events from diverse hardware are delivered in a consistent format.

#### Acceptance Criteria

1. THE Input_Manager SHALL read raw events from kernel input device nodes and normalise them into a unified event type (`KeyEvent`, `PointerMotion`, `PointerButton`, `TouchDown`, `TouchUp`).
2. THE Input_Manager SHALL apply keyboard layout mapping (at minimum US QWERTY) to translate hardware scan codes to Unicode code points.
3. WHEN a pointer device reports relative motion, THE Input_Manager SHALL apply configurable acceleration and deliver the resulting absolute position to the Compositor.
4. THE Input_Manager SHALL support hot-plugging of input devices: WHEN a new input device is attached, THE Input_Manager SHALL begin delivering its events within 500 ms.
5. THE Input_Manager SHALL deliver input events to the Compositor via a Unix socket with latency under 5 ms from kernel event to Compositor receipt.

---

### Requirement 41: Display Manager (greetd-Style)

**User Story:** As a user, I want a display manager, so that I can log in graphically and have a desktop session started on my behalf.

#### Acceptance Criteria

1. THE Display_Manager SHALL present a graphical login screen using the Wayland compositor before any user session is started.
2. WHEN a user submits credentials, THE Display_Manager SHALL authenticate them against the system user database and, on success, start a desktop session as that user.
3. WHEN authentication fails, THE Display_Manager SHALL display an error message and allow the user to retry without restarting the compositor.
4. WHEN a desktop session exits, THE Display_Manager SHALL return to the login screen.
5. THE Display_Manager SHALL run as an unprivileged daemon and use a privileged helper (with `CAP_SETUID`) only for the credential verification and session launch steps.

---

### Requirement 42: Basic Desktop Shell

**User Story:** As a user, I want a minimal desktop shell, so that I have a usable graphical environment with a taskbar, application launcher, and window management.

#### Acceptance Criteria

1. THE Desktop_Shell SHALL display a taskbar showing the names of all open toplevel windows and allow the user to switch focus by clicking a taskbar entry.
2. THE Desktop_Shell SHALL provide an application launcher that lists installed applications from `/usr/share/turnix/applications/` and launches the selected application on activation.
3. THE Desktop_Shell SHALL support tiling window management: WHEN the user presses a configurable keyboard shortcut, THE Desktop_Shell SHALL tile the focused window to the left or right half of the screen.
4. THE Desktop_Shell SHALL display a system clock in the taskbar updated every second.
5. THE Desktop_Shell SHALL consume less than 50 MB of resident memory when idle with no application windows open.


---

## Phase 8 — CI/CD and Validation

### Requirement 43: Boot-to-Shell Gating (30 QEMU Cold Boots)

**User Story:** As a project maintainer, I want an automated boot-to-shell gate, so that no commit that breaks the boot path can merge to the integration branch.

#### Acceptance Criteria

1. THE CI_Gate SHALL execute 30 sequential QEMU cold-boot cycles for every pull request targeting the integration branch.
2. WHEN each boot cycle completes, THE CI_Gate SHALL verify that the shell prompt is reachable within 30 seconds of QEMU start.
3. IF any of the 30 boot cycles fails to reach the shell prompt, THE CI_Gate SHALL mark the pull request as failed and block the merge.
4. THE CI_Gate SHALL report the pass rate (e.g., 28/30) and the serial log of any failed boot in the pull request status.
5. THE CI_Gate SHALL complete all 30 boot cycles within 20 minutes of being triggered.

---

### Requirement 44: Driver Test Suite

**User Story:** As a kernel developer, I want an automated driver test suite, so that driver regressions are caught before they reach the integration branch.

#### Acceptance Criteria

1. THE CI_Gate SHALL run a driver test suite in QEMU that exercises each driver registered in the Driver_Framework with at least one functional test.
2. THE driver test suite SHALL include tests for: PCIe enumeration (device count matches expected), NVMe read/write round-trip, VirtIO_Net send/receive loopback, XHCI device enumeration, and GPU framebuffer pixel write/read.
3. WHEN a driver test fails, THE CI_Gate SHALL report the failing driver name, test case, and QEMU serial log.
4. THE driver test suite SHALL achieve at least 80% line coverage of each driver module as measured by LLVM source-based coverage.
5. THE driver test suite SHALL complete within 10 minutes on the CI runner.

---

### Requirement 45: Security Regression Tests (CIS-Equivalent)

**User Story:** As a security engineer, I want automated security regression tests, so that security properties are continuously verified and regressions are caught immediately.

#### Acceptance Criteria

1. THE CI_Gate SHALL run a security regression suite that verifies: ASLR is active (process base addresses differ across 10 consecutive exec calls), W^X is enforced (no page is simultaneously writable and executable), stack canaries are present in kernel and libturnix binaries, and seccomp filters are inherited across fork.
2. THE CI_Gate SHALL run a CIS-equivalent checklist of at least 20 security controls and report pass/fail for each.
3. WHEN any security regression test fails, THE CI_Gate SHALL block the merge and label the pull request with `security-regression`.
4. THE security regression suite SHALL complete within 15 minutes on the CI runner.
5. THE CI_Gate SHALL generate a security report artifact for each run that can be archived and compared across releases.

---

### Requirement 46: Performance Benchmarks

**User Story:** As a project maintainer, I want automated performance benchmarks, so that performance regressions are detected before they reach users.

#### Acceptance Criteria

1. THE CI_Gate SHALL measure cold-boot time from QEMU start to shell prompt and fail the build if it exceeds 5 seconds on an NVMe-backed QEMU instance.
2. THE CI_Gate SHALL measure idle RAM consumption after boot-to-shell and fail the build if it exceeds 1.5 GB.
3. THE CI_Gate SHALL measure syscall round-trip latency (a `getpid` loop) and fail the build if the median latency exceeds 1 microsecond.
4. THE CI_Gate SHALL measure VFS read throughput on a 100 MB file and fail the build if it falls below 500 MB/s on a RAM-backed tmpfs.
5. THE CI_Gate SHALL store benchmark results as time-series data and display a trend graph in the pull request status for the last 30 runs.

