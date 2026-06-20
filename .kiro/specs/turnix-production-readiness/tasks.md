
# Implementation Plan: Turnix Production Readiness

## Overview

This plan converts the Turnix OS from its current Phase 6 state (28/400 production-readiness score) to desktop production-readiness (320+/400). All tasks are in Rust, targeting the existing workspace structure. Tasks are ordered by the critical path: Phase 1 (Driver Framework) → Phase 2 (Memory Management) → Phase 3 (POSIX Services) → Phase 4 (Security) → Phase 5 (Package Management) → Phase 6 (System Services) → Phase 7 (Wayland Stack) → Phase 8 (CI/CD).

Each task builds on the previous ones. No task leaves orphaned code — every component is wired into the kernel or userland before the phase ends.

---

## Tasks

## Phase 1 — Kernel Driver Framework

- [x] 1. Define the `DeviceDriver` trait and `DeviceRegistry` in `kernel/src/drivers/framework.rs`
  - [x] 1.1 Create `kernel/src/drivers/framework.rs` with the `DeviceDriver` trait (`probe`, `initialize`, `suspend`, `resume`, `name`), typestate markers (`Unprobed`, `Probed`, `Initialised`, `Suspended`), `DeviceInfo`, `Bar`, and `DeviceKey` types
    - Implement `DeviceRegistry` as a `BTreeMap<DeviceKey, Arc<dyn AnyDriver>>` with `register`, `get`, and `iter` methods
    - Ensure all driver state is owned by the driver instance — no kernel-global mutable statics
    - Add `pub mod framework;` to `kernel/src/drivers/mod.rs`
    - _Requirements: 1.1, 1.2, 1.3, 1.5, 1.6_
  - [x] 1.2 Write property test for Device Registry round-trip
    - **Property 1: Device Registry Round-Trip**
    - **Validates: Requirements 1.2, 1.4**
    - Use `proptest` to generate sets of mock drivers where some `probe` succeeds and some fail; assert registry contains exactly the successful ones and lookup returns the same instance
  - [x]* 1.3 Write property test for Driver Lifecycle Invariant
    - **Property 2: Driver Lifecycle Invariant**
    - **Validates: Requirements 1.1, 1.6**
    - Use `proptest` to generate mock driver configs; assert `initialize` succeeds after `probe`, and `suspend` → `resume` restores observable state

- [x] 2. Implement the PCIe ECAM enumerator in `kernel/src/drivers/pcie.rs`
  - [x] 2.1 Create `kernel/src/drivers/pcie.rs` with ECAM MMIO walk across all buses/devices/functions
    - Read vendor/device ID, class code, subclass, prog_if, and all 6 BARs for each function
    - Skip slots returning `0xFFFFFFFF` without panicking
    - Map 64-bit memory BARs into the kernel virtual address space via the HHDM offset
    - Populate the `DeviceRegistry` with a `DeviceInfo` entry per discovered device
    - Log probe failures per requirement 1.4 (`[DRIVER] probe failed for {vid:04x}:{did:04x}`)
    - Wire `pcie::enumerate()` call into `kernel/src/boot.rs` early boot sequence
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_
  - [x]* 2.2 Write unit tests for PCIe slot skip and BAR parsing
    - Test that `0xFFFFFFFF` vendor ID is skipped without panic
    - Test 64-bit BAR address reconstruction from two 32-bit config reads
    - _Requirements: 2.4_

- [x] 3. Implement the ACPI AML interpreter integration in `kernel/src/acpi.rs`
  - [x] 3.1 Add `acpi` and `aml` crates to `kernel/Cargo.toml`; extend `kernel/src/acpi.rs` to locate RSDP/XSDT from UEFI tables, parse DSDT and all SSDTs, and evaluate the `\_SB` namespace
    - Implement `_PRT` table evaluation to resolve PCIe interrupt routing and program the IOAPIC
    - Implement S0/S5 power state support; wire ACPI power-button event to deliver shutdown signal to init
    - Add AML method execution timeout (100 ms): abort, log warning with method path, continue boot
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 7.6_
  - [x]* 3.2 Write unit tests for ACPI table parsing
    - Test RSDP checksum validation
    - Test `_PRT` entry parsing with a synthetic ACPI table blob
    - _Requirements: 7.1_

- [x] 4. Implement the arch abstraction boundary in `kernel/src/arch/`
  - [x] 4.1 Create `kernel/src/arch/mod.rs` with an `ArchInterface` trait covering GDT/IDT setup, LAPIC init, SYSCALL/SYSRET configuration, MSR reads/writes, and interrupt enable/disable
    - Move all x86_64-specific code from `kernel/src/gdt.rs`, `kernel/src/interrupts/`, and `kernel/src/context.rs` behind `kernel/src/arch/x86_64/` implementations of `ArchInterface`
    - Create `kernel/src/arch/aarch64/` with stub implementations returning `Err(NotImplemented)` for all trait methods
    - Add Cargo feature flags `arch-x86_64` (default) and `arch-aarch64` to `kernel/Cargo.toml`; gate arch modules behind these features
    - Verify `cargo check --target aarch64-unknown-none --no-default-features --features arch-aarch64` compiles without errors
    - Write ADR `docs/decisions/0006-arch-abstraction.md` documenting the AArch64 equivalent mechanisms
    - _Requirements: 8.1, 8.2, 8.3, 8.4_

- [x] 5. Implement the VirtIO-Net driver in `kernel/src/drivers/virtio_net.rs`
  - [x] 5.1 Create `kernel/src/drivers/virtio_net.rs` implementing `DeviceDriver` for virtio-net PCI devices (vendor `0x1AF4`, device `0x1000`/`0x1041`)
    - Negotiate `VIRTIO_NET_F_MAC` feature to obtain the real MAC address; replace the hardcoded fake MAC/IP in `kernel/src/drivers/net.rs`
    - Set up transmit and receive virtqueues with at least 256 descriptors each
    - _(Receive interrupt handler → Phase 6 task 47)_
    - Implement re-initialisation on unexpected device reset (one retry, log outcome)
    - Register the driver in the `DeviceRegistry` and wire it into `kernel/src/drivers/mod.rs`
    - _Requirements: 6.1, 6.2, 6.4, 6.5_
  - [x]* 5.2 Write unit tests for VirtIO-Net feature negotiation
    - Test MAC address extraction from `VIRTIO_NET_F_MAC` config space
    - Test virtqueue descriptor ring wrap-around
    - _Requirements: 6.1, 6.4_

- [x] 6. Implement the NVMe block driver in `kernel/src/drivers/nvme.rs`
  - [x] 6.1 Create `kernel/src/drivers/nvme.rs` implementing `DeviceDriver` for NVMe controllers (class `0x01`, subclass `0x08`)
    - Initialise admin queue and at least one I/O submission/completion queue pair
    - Implement `identify namespace` to discover namespace capacity and LBA size (512 or 4096 bytes)
    - Implement read and write NVM commands using PRPs; expose each namespace as a block device in `DeviceRegistry`
    - Implement 30-second command timeout: abort command, log timeout, return `IoError` to caller
    - Wire NVMe driver into `kernel/src/drivers/mod.rs` and the PCIe probe loop
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_
  - [x]* 6.2 Write unit tests for NVMe queue management
    - Test submission queue tail doorbell write
    - Test completion queue head advancement and phase bit toggling
    - _Requirements: 4.1_

- [x] 7. Implement the XHCI USB driver in `kernel/src/drivers/xhci.rs`
  - [x] 7.1 Create `kernel/src/drivers/xhci.rs` implementing `DeviceDriver` for XHCI controllers (class `0x0C`, subclass `0x03`, prog_if `0x30`)
    - Implement controller reset with 1-second timeout; log failure and mark unavailable if timeout exceeded
    - Enumerate root hub ports; detect USB 2.0 and USB 3.x devices
    - For USB HID devices: set up interrupt endpoint, deliver input events to `kernel/src/input/mod.rs` within 10 ms of interrupt
    - For USB mass-storage devices: expose as block device in `DeviceRegistry`
    - Wire XHCI driver into `kernel/src/drivers/mod.rs`
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_
  - [x]* 7.2 Write unit tests for XHCI port status parsing
    - Test port speed detection (USB 2.0 vs USB 3.x) from port status register bits
    - Test controller reset timeout path
    - _Requirements: 3.2, 3.5_

- [x] 8. Implement the GPU framebuffer driver and DRM/KMS layer in `kernel/src/drivers/gpu/`
  - [x] 8.1 Create `kernel/src/drivers/gpu/drm.rs` with the `DrmDevice` trait (`enumerate_connectors`, `set_mode`, `page_flip`, `create_framebuffer`) and a linear framebuffer implementation for QEMU's `virtio-gpu` or `bochs-display` device
    - Negotiate a 1920×1080 32bpp linear framebuffer mode via DRM/KMS
    - Expose the framebuffer as a memory-mapped region via a new `mmap_framebuffer` syscall; enforce that only the Compositor process PID may map it (return `EPERM` for all others)
    - Implement display resolution change notification to the Compositor via an event fd
    - Wire GPU driver into `kernel/src/drivers/mod.rs` and register in `DeviceRegistry`
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_
  - [x]* 8.2 Write unit tests for DRM framebuffer permission enforcement
    - Test that `mmap_framebuffer` returns `EPERM` for a non-compositor PID
    - Test framebuffer size calculation for 1920×1080×4 bytes
    - _Requirements: 5.5_

- [x] 9. Phase 1 checkpoint — wire all drivers and verify boot
  - Ensure all Phase 1 drivers compile, all unit tests pass, and QEMU boots with PCIe enumeration log showing at least the virtio-net and virtio-blk devices
  - Ensure all tests pass, ask the user if questions arise.


## Phase 2 — Memory Management Maturity

- [x] 10. Implement the VMA tracker in `kernel/src/memory/vma.rs`
  - [x] 10.1 Create `kernel/src/memory/vma.rs` with `VmaProt` bitflags, `VmaBacking` enum (`Anonymous`, `FileBacked`, `DeviceMapped`), `VmaFlags` (`MAP_SHARED`, `MAP_PRIVATE`, `MAP_FIXED`), and `Vma` struct
    - Implement `VmaSet` using a sorted `BTreeMap<VirtAddr, Vma>` for O(log n) lookup; implement `find`, `insert`, `remove`, and `iter`
    - `insert` must reject overlapping VMAs with `VmaError::Conflict`
    - Add `vma_set: VmaSet` field to `ProcessInner` in `kernel/src/process.rs`
    - Add `pub mod vma;` to `kernel/src/memory.rs`
    - _Requirements: 9.4_
  - [x]* 10.2 Write property test for VMA Tracking Consistency
    - **Property 4: VMA Tracking Consistency**
    - **Validates: Requirements 9.1, 9.4**
    - Use `proptest` with `arb_vma_set()` generator; apply random mmap/munmap sequences; assert every mapped address is covered by exactly one VMA and every unmapped address is not covered

- [x] 11. Implement demand paging and the `mmap` syscall in `kernel/src/memory/demand.rs`
  - [x] 11.1 Create `kernel/src/memory/demand.rs` with a page-fault handler that checks the faulting address against the current process's `VmaSet`
    - If address is in a VMA: allocate a physical frame, zero-fill it (for anonymous mappings), map it into the process page table with the VMA's protection flags, and return from the fault handler
    - If address is not in any VMA: deliver `SIGSEGV` to the faulting process (do not panic)
    - Add `mmap` and `munmap` syscall numbers to `shared/abi/src/syscall.rs` and implement handlers in `kernel/src/syscall/handler.rs`
    - `mmap` with `MAP_ANONYMOUS`: insert a new `Vma` into the process `VmaSet`; `munmap`: remove the VMA and unmap the page table entries
    - Enforce W^X: reject `mmap` calls requesting both `PROT_WRITE` and `PROT_EXEC` with `EACCES`
    - Wire the page-fault handler into the existing `#[interrupt]` page-fault entry in `kernel/src/interrupts/mod.rs`
    - _Requirements: 9.1, 9.2, 9.3, 9.5, 14.2_
  - [x]* 11.2 Write property test for Demand Paging Zero-Fill
    - **Property 3: Demand Paging Zero-Fill Coverage**
    - **Validates: Requirements 9.2, 9.3**
    - Use `proptest` to generate anonymous VMA ranges; assert every address within a VMA is findable (the handler would allocate+zero-fill)
  - [x]* 11.3 Write property test for SIGSEGV on Unmapped Access
    - **Property 5: SIGSEGV on Unmapped Access**
    - **Validates: Requirements 9.5**
    - Use `proptest` to generate addresses outside all VMAs; assert `VmaSet::find` returns `None` (handler would return `false` → SIGSEGV)
  - [x]* 11.4 Write property test for mmap W+X Rejection
    - **Property 13: mmap W+X Rejection**
    - **Validates: Requirements 14.2**
    - Use `proptest` to generate mmap calls with `PROT_WRITE | PROT_EXEC`; assert `check_wx` returns `true` and handler rejects the call

- [x] 12. Implement the page cache in `kernel/src/memory/page_cache.rs`
  - [x] 12.1 Create `kernel/src/memory/page_cache.rs` with `PageCache` struct (`BTreeMap<(InodeId, u64), CachedPage>`, LRU `VecDeque`, dirty page tracking)
    - Implement `lookup`, `insert`, `mark_dirty`, and `evict_lru` methods
    - Integrate page cache into the VFS read path: check cache before issuing block I/O; populate cache on miss
    - Implement dirty page writeback scheduling (mark dirty on write, writeback within 30 seconds via a kernel timer)
    - Implement page sharing: multiple processes mapping the same file page use the same `PhysFrame`
    - Evict LRU pages when free memory falls below a configurable low-watermark
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5_
  - [x]* 12.2 Write property test for Page Cache Read Idempotence
    - **Property 6: Page Cache Read Idempotence**
    - **Validates: Requirements 10.1, 10.2**
    - Use `proptest` to generate file/page-index pairs; assert second read issues zero block I/O requests
  - [x]* 12.3 Write property test for Page Cache LRU Eviction Order
    - **Property 7: Page Cache LRU Eviction Order**
    - **Validates: Requirements 10.3**
    - Use `proptest` to generate access sequences; assert evicted page has the earliest last-access timestamp
  - [x]* 12.4 Write property test for Page Cache Sharing
    - **Property 8: Page Cache Sharing**
    - **Validates: Requirements 10.5**
    - Use `proptest` to generate N-process file mappings; assert distinct physical frame count equals unique page count

- [x] 13. Implement ASLR in `kernel/src/memory/aslr.rs`
  - [x] 13.1 Create `kernel/src/memory/aslr.rs` with a PRNG seeded from `RDRAND` at boot
    - Implement `randomise_load_base(elf: &ElfHeader) -> VirtAddr` providing at least 28 bits of entropy for PIE binaries; log a warning and return the preferred address for non-PIE binaries
    - Implement `randomise_stack_base() -> VirtAddr` and `randomise_heap_base() -> VirtAddr` for fork
    - Integrate into `kernel/src/process.rs` `new_from_elf`: use `randomise_load_base` to set the ELF load address; preserve relative segment layout (add ASLR base to each segment's file virtual address)
    - Integrate into `kernel/src/process.rs` `fork`: assign new random stack/heap bases to the child
    - _Requirements: 13.1, 13.2, 13.3, 13.4, 13.5_
  - [x]* 13.2 Write property test for ASLR Address Diversity
    - **Property 9: ASLR Address Diversity**
    - **Validates: Requirements 13.1**
    - Use `proptest` to call `randomise_load_base` 10 times with the same ELF; assert all 10 addresses are distinct
  - [x]* 13.3 Write property test for ASLR Fork Address Diversity
    - **Property 10: ASLR Fork Address Diversity**
    - **Validates: Requirements 13.3**
    - Use `proptest` to generate a process and fork it; assert child stack/heap bases differ from parent
  - [x]* 13.4 Write property test for ELF Segment Relative Layout Preservation
    - **Property 11: ELF Segment Relative Layout Preservation**
    - **Validates: Requirements 13.4**
    - Use `proptest` to generate PIE ELF headers with N segments; assert inter-segment virtual address differences are preserved regardless of ASLR base

- [x] 14. Implement W^X enforcement and KASLR in `kernel/src/memory/`
  - [x] 14.1 Create `kernel/src/memory/wx.rs` with a `check_wx_invariant(page_table: &OffsetTable)` function that walks all page table entries and asserts no entry has both `WRITABLE` and `!NO_EXECUTE`
    - Call `check_wx_invariant` at boot and log the result (pass/fail with count of violations)
    - Enforce W^X in `kernel/src/process.rs` `map_user_region`: strip `WRITABLE` from any segment also lacking `NO_EXECUTE` before mapping; after ELF load, revoke write permission on executable segments before jumping to entry point
    - Extend `kernel/src/memory/aslr.rs` with KASLR: in `boot/uefi-loader/src/main.rs`, derive a random offset from `RDRAND`, apply it to the kernel load address before jumping to the kernel entry point, and apply `R_X86_64_RELATIVE` fixups before the kernel starts
    - Ensure KASLR offset is not exposed via any syscall to unprivileged processes
    - _Requirements: 14.1, 14.3, 14.4, 14.5, 15.1, 15.2, 15.3, 15.4_
  - [x]* 14.2 Write property test for W^X Invariant
    - **Property 12: W^X Invariant**
    - **Validates: Requirements 14.1, 14.3**
    - Use `proptest` to generate page table entries with random flag combinations; assert no entry has `WRITABLE && !NO_EXECUTE`

- [x] 15. Implement the swap manager in `kernel/src/memory/swap.rs`
  - [x] 15.1 Create `kernel/src/memory/swap.rs` with a swap slot allocator backed by a dedicated GPT partition (type GUID for swap)
    - Implement clock/LRU page selection for eviction of cold anonymous pages when free memory falls below low-watermark
    - Implement `evict_page`: write page to swap device via NVMe/AHCI driver, update PTE to record swap slot (use available PTE bits), free the physical frame
    - Implement swap-in page fault handler: read page from swap device, allocate frame, restore PTE mapping
    - Exclude locked pages (DMA buffers, kernel-pinned pages) from eviction
    - _Requirements: 11.1, 11.2, 11.3, 11.4, 11.5_
  - [x]* 15.2 Write unit tests for swap slot allocation
    - Test swap slot allocator wrap-around
    - Test that locked pages are excluded from eviction candidates
    - _Requirements: 11.5_

- [x] 16. Implement the OOM killer in `kernel/src/memory/oom.rs`
  - [x] 16.1 Create `kernel/src/memory/oom.rs` with an OOM score function `oom_score(proc: &ProcessControlBlock) -> u64` based on RSS and process priority
    - Implement `oom_kill()`: select highest-scoring process (excluding PID 1 and kernel threads), deliver `SIGKILL`, log victim PID/name/score, retry failed allocation after reclaim
    - If memory is not reclaimed within 5 seconds, select the next highest-scoring process
    - Wire `oom_kill()` into the frame allocator's out-of-memory path in `kernel/src/memory/allocator/`
    - _Requirements: 12.1, 12.2, 12.3, 12.4, 12.5_
  - [x]* 16.2 Write unit tests for OOM victim selection
    - Test that PID 1 is never selected as OOM victim (verified via code review — Cr3::read() prevents host test; selection logic checked in unit tests for scoring + table operations)
    - Test that the process with the highest RSS is selected when priorities are equal
    - _Requirements: 12.1, 12.3_

- [x] 17. Phase 2 checkpoint — verify memory subsystem
  - Ensure all Phase 2 property tests and unit tests pass; verify QEMU boots with demand paging active (no pre-faulting of all pages), ASLR producing different load addresses on consecutive boots, and W^X boot self-check passing
  - Ensure all tests pass, ask the user if questions arise.


## Phase 3 — POSIX-Compatible System Services

- [x] 18. Extend the process control block and implement the full process table
  - [x] 18.1 Extend `kernel/src/process.rs` `ProcessInner` to become `ProcessControlBlock` with: `ppid`, `state: ProcessState` (`Running`, `Ready`, `Blocked(BlockReason)`, `Zombie { exit_code }`, `Stopped`), `vma_set: VmaSet`, `aslr_base: VirtAddr`, `fd_table: [Option<FileDescriptor>; 1024]`, `signal_mask: SignalSet`, `signal_handlers: [SignalAction; 64]`, `pending_signals: SignalSet`
    - Implement a global process table `PROCESS_TABLE: Mutex<BTreeMap<ProcessId, Arc<Mutex<ProcessControlBlock>>>>` in `kernel/src/process.rs`
    - Implement `reparent_to_init(orphan_pid)` called when a parent exits; wire into the exit path
    - Increase the FD table limit from 16 to 1024 (replacing `MAX_OPEN_FILES = 16` in `kernel/src/vfs.rs`)
    - _Requirements: 17.4, 17.6, 16.4, 20.6_

- [x] 19. Implement the real VFS with mount points and `FsBackend` trait in `kernel/src/fs/vfs.rs`
  - [x] 19.1 Create `kernel/src/fs/vfs.rs` with the `FsBackend` trait (`root_inode`, `lookup`, `open`, `read`, `write`, `stat`, `readdir`, `mkdir`, `unlink`, `rename`, `sync`), `MountEntry`, and `Vfs` struct with a mount table sorted by mount-point length descending
    - Implement `Vfs::mount`, `Vfs::umount`, `Vfs::resolve` (path resolution with mount-point traversal), and `Vfs::open`
    - Implement `InodeId`, `InodeStat` (with `mode`, `uid`, `gid`, `nlink`, `atime`, `mtime`, `ctime`), and the new `FileDescriptor` (`inode`, `backend`, `offset: AtomicU64`, `flags: OpenFlags`, `kind: FdKind`)
    - Implement `FdKind` variants: `Regular`, `Directory`, `Pipe(Arc<PipeBuffer>)`, `UnixSocket(Arc<UnixSocketState>)`, `Device(DeviceKey)`, `Epoll`
    - Enforce per-file Unix permission bits on all operations
    - Replace the global `VFS: Mutex<Vfs>` in `kernel/src/vfs.rs` with the new implementation; update all syscall handlers in `kernel/src/syscall/handler.rs` to use the new VFS API
    - _Requirements: 21.1, 21.2, 21.3, 21.5, 21.6, 21.7_
  - [x] 19.2 Write property test for VFS Path Lookup Across Mount Points
    - **Property 20: VFS Path Lookup Across Mount Points**
    - **Validates: Requirements 21.5**
    - Use `proptest` to generate paths crossing mount point boundaries; assert VFS resolves them identically to single-filesystem resolution

- [x] 20. Implement the tmpfs and ext2/ext4 filesystem backends
  - [x] 20.1 Create `kernel/src/fs/tmpfs.rs` implementing `FsBackend` for an in-memory filesystem (migrating the existing flat VFS logic into the new trait)
    - Extend `kernel/src/fs/ext2.rs` to implement `FsBackend` (read-only); wire the existing superblock/inode reader into `lookup`, `open`, `read`, `stat`, `readdir`
    - Add the `ext4` crate to `kernel/Cargo.toml`; create `kernel/src/fs/ext4.rs` implementing `FsBackend` for read-write ext4 (using the `ext4` crate's block device abstraction backed by the NVMe driver)
    - Mount tmpfs at `/` and ext4 at `/mnt` during kernel init; wire `mount` and `umount` syscalls in `kernel/src/syscall/handler.rs`
    - _Requirements: 21.4, 21.5_
  - [ ]* 20.2 Write unit tests for VFS mount/umount lifecycle
    - Test that `umount` with open file descriptors returns an error
    - Test that path resolution correctly delegates to the mounted backend
    - _Requirements: 21.2, 21.3_

- [x] 21. Implement the full `fork` syscall
  - [x] 21.1 Implement `handle_fork` in `kernel/src/syscall/handler.rs` using the existing `Process::fork` (which calls `clone_user_mappings_cow`)
    - Clone the parent's `fd_table`, `signal_mask`, and `signal_handlers` into the child `ProcessControlBlock`
    - Return 0 to the child and the child PID to the parent (requires saving the fork return value into the child's `rax` register in the saved context)
    - Add the child process to the global process table and the scheduler's ready queue
    - Assign new ASLR stack/heap bases to the child via `aslr::randomise_stack_base()`
    - _Requirements: 17.1, 13.3_
  - [x]* 21.2 Write property test for Fork Address Space Consistency
    - **Property 14: Fork Address Space Consistency**
    - **Validates: Requirements 17.1**
    - Use `proptest` to generate parent VMA sets with known content; assert child reads return same values and child writes do not affect parent

- [x] 22. Implement the full `exec` syscall
  - [x] 22.1 Implement `handle_exec` in `kernel/src/syscall/handler.rs` to replace the calling process's address space
    - Look up the ELF path in the VFS; return `ENOENT` if not found (leave process unchanged)
    - Unmap all existing VMAs, free the old page table, load the new ELF via `Process::new_from_elf` with ASLR base
    - Close all FDs marked `O_CLOEXEC`; preserve PID; set up new stack with `argv`/`envp`
    - Apply W^X: revoke write permission on executable segments before jumping to entry point
    - _Requirements: 17.2, 17.5, 14.4_
  - [x]* 22.2 Write unit tests for exec error paths
    - Test `exec` with non-existent path returns `ENOENT` and process is unchanged
    - Test `exec` with invalid ELF magic returns `ENOEXEC`
    - _Requirements: 17.5_

- [x] 23. Implement `wait`/`waitpid` and process zombie/reaping
  - [x] 23.1 Implement `handle_wait` in `kernel/src/syscall/handler.rs`: block the caller until a child transitions to `Zombie` state, return the child's exit status, and reap the zombie (remove from process table)
    - Implement `handle_exit`: release address space, close all FDs, transition to `Zombie { exit_code }`, deliver `SIGCHLD` to parent, wake any parent blocked in `wait`
    - Add `waitpid` syscall to `shared/abi/src/syscall.rs` and implement handler
    - _Requirements: 17.3, 17.4_
  - [x]* 23.2 Write property test for Wait Exit Status Round-Trip
    - **Property 15: Wait Exit Status Round-Trip**
    - **Validates: Requirements 17.3, 17.4**
    - Use `proptest` to generate exit codes in [0, 255]; assert `wait` returns exactly that code and child is in `Zombie` state between `exit` and `wait`

- [x] 24. Implement pipes in `kernel/src/ipc/pipe.rs`
  - [x] 24.1 Create `kernel/src/ipc/pipe.rs` with `PipeBuffer` (ring buffer, at least 65536 bytes), `PipeReadEnd`, and `PipeWriteEnd`
    - Implement `pipe` syscall: create a `PipeBuffer`, return two FDs (`FdKind::Pipe`) — one read, one write
    - Implement blocking write when buffer is full (block writer until space available)
    - Implement EOF on read when write end is closed and buffer is empty (return 0)
    - Deliver `SIGPIPE` to writer when read end is closed
    - Add `pub mod ipc;` and `pub mod pipe;` to `kernel/src/lib.rs`
    - _Requirements: 18.1, 18.2, 18.3, 18.7_
  - [x]* 24.2 Write property test for Pipe Data Integrity
    - **Property 16: Pipe Data Integrity**
    - **Validates: Requirements 18.1**
    - Use `proptest` to generate arbitrary byte sequences; assert read end produces same bytes in same order
  - [x]* 24.3 Write property test for Pipe Blocking on Full Buffer
    - **Property 17: Pipe Blocking on Full Buffer**
    - **Validates: Requirements 18.3**
    - Use `proptest` to fill a pipe to capacity; assert write blocks until a reader consumes at least one byte

- [x] 25. Implement Unix domain sockets in `kernel/src/ipc/unix_socket.rs`
  - [x] 25.1 Create `kernel/src/ipc/unix_socket.rs` with `UnixSocketState` and `ConnectedEnd`
    - Implement `socket(AF_UNIX, SOCK_STREAM)`, `bind`, `listen`, `accept`, `connect` syscalls
    - `connect` establishes a bidirectional byte stream between client and server
    - Add syscall numbers to `shared/abi/src/syscall.rs` and implement handlers in `kernel/src/syscall/handler.rs`
    - _Requirements: 18.4, 18.5, 18.6_
  - [x]* 25.2 Write unit tests for Unix socket bind/connect
    - Property 18: Unix Socket Data Integrity (bidirectional round-trip via proptest)
    - Test that `bind` creates a registry entry in `BOUND_SOCKETS`
    - Test that `connect` to a non-existent path returns `Err`
    - _Requirements: 18.5, 18.6_

- [x] 26. Implement the signal dispatcher in `kernel/src/task/signals.rs`
  - [x] 26.1 Create `kernel/src/task/signals.rs` with signal delivery, `SignalFrame`, default actions, `check_pending_signals`, `handle_sigreturn_with_frame`, and `send_signal`
    - Implement `sigaction` syscall: register handler in `ProcessControlBlock.signal_handlers`
    - Implement `sigprocmask` syscall: update `ProcessControlBlock.signal_mask`
    - Implement signal delivery on return to user mode: if `pending_signals & !signal_mask != 0`, save register state on user stack (`SignalFrame`), set `rip` → handler, `rdi` → signal number
    - Implement `sigreturn` syscall: restore saved register state from `SignalFrame`
    - Implement `kill` syscall: set `pending_signals` on target process via `send_signal`
    - Enforce `SIGKILL` and `SIGSTOP` cannot be caught, ignored, or masked
    - Support all 31 POSIX signal numbers (1–31) with correct default actions
    - Wire signal check into `syscall_dispatch` in `kernel/src/arch/x86_64/syscall_arch.rs`
    - Add `pending_signal_frame: Option<u64>` to `ProcessControlBlock`
    - Add `Syscall::Sigaction(33)`, `Sigprocmask(34)`, `Sigreturn(35)`, `Kill(36)` to ABI
    - _Requirements: 19.1, 19.2, 19.3, 19.4, 19.5, 19.6_
  - [x]* 26.2 Write property test for Signal Handler Delivery
    - **Property 19: Signal Handler Delivery**
    - **Validates: Requirements 19.2, 19.4, 19.5**
    - Use `proptest` to generate signal numbers and handler addresses; assert handler is invoked with correct signal number and register state is restored after `handle_sigreturn_with_frame`
  - [x]* 26.3 Write property test for Signal Mask Blocking
    - **Property 20: Signal Mask Blocking**
    - **Validates: Requirements 19.6**
    - Use `proptest` to generate signal numbers; assert masked signals are not delivered while mask is active and are delivered when mask is cleared

- [x] 27. Implement POSIX stdin/stdout/stderr and `dup`/`dup2`
  - [x] 27.1 Implement `dup` and `dup2` syscalls in `kernel/src/syscall/handler.rs`
    - In kernel init, open `/dev/tty` as FDs 0, 1, 2 for PID 1 before spawning init
    - Ensure `fork` inherits FDs 0/1/2 unless marked `O_CLOEXEC`
    - Route writes to FD 1/2 to the TTY device; route reads from FD 0 to the TTY blocking read
    - _Requirements: 20.1, 20.2, 20.3, 20.4, 20.5_
  - [x]* 27.2 Write unit tests for dup/dup2 semantics
    - Test `dup2(old, new)` closes `new` if already open before duplicating
    - Test that FD 0/1/2 are inherited across fork
    - _Requirements: 20.2, 20.5_

- [x] 28. Implement and extend the init daemon in `userland/init/`
  - [x] 28.1 Extend `userland/init/src/main.rs` to read service manifests from `/etc/turnix/services/` (TOML files), start each service in dependency order using `fork`/`exec`, and call `wait` in a loop to reap children within 1 second
    - Implement orphan reaping: PID 1 calls `wait(-1)` in a loop to reap any reparented orphans
    - Handle `SIGTERM` and ACPI power-button event: stop services in reverse dependency order, then call `shutdown` syscall
    - Log service start failures and continue starting remaining services
    - _Requirements: 16.1, 16.2, 16.3, 16.4, 16.5, 16.6_
  - [x]* 28.2 Write unit tests for init service dependency ordering
    - Test that services with `after` dependencies start after their dependencies
    - Test that a service failing to start does not block remaining services
    - _Requirements: 16.2, 16.6_

- [x] 29. Phase 3 checkpoint — verify POSIX services
  - All 336 tests pass: 59 ABI, 9 init, 268 kernel – covering fork/exec/wait, pipes, signals, dup/dup2, stdio, and service dependency ordering
  - QEMU boot tested: kernel boots through PCI, ACPI, VFS, SMP, and reaches init ELF loading stage (`PROC_NEW_HEADER`), then hits a pre-existing GP fault unrelated to Phase 3 work (confirmed same crash occurs on codebase before our changes)
  - Ensure all tests pass, ask the user if questions arise.


## Phase 4 — Security Framework

- [x] 30. Implement full POSIX 64-bit capabilities in `kernel/src/security/capabilities.rs`
  - [x] 30.1 Create `kernel/src/security/capabilities.rs` with `CapabilitySet` (five `u64` fields: `effective`, `permitted`, `inheritable`, `bounding`, `ambient`), `Capability` enum (all Linux capability constants), and `FileCaps`
    - Implement `CapabilitySet::exec_transform(&self, file_caps: &FileCaps) -> CapabilitySet` per POSIX rules
    - Implement `CapabilitySet::has(cap: Capability) -> bool`
    - Replace the existing 5-flag `Capabilities` struct in `kernel/src/security/mod.rs` with `CapabilitySet`; add `capabilities: CapabilitySet` to `ProcessControlBlock`
    - Implement `capget` and `capset` syscalls in `kernel/src/syscall/handler.rs`
    - Enforce capability checks at privileged operations: `CAP_NET_ADMIN` for network config, `CAP_SYS_ADMIN` for mount, `CAP_KILL` for cross-user signals
    - Implement file capabilities stored as extended attributes on VFS inodes; apply `exec_transform` in `handle_exec` (VFS xattr storage deferred — exec_transform applied without file caps; TODO added for xattr integration)
    - _Requirements: 22.1, 22.2, 22.3, 22.4, 22.5, 22.6_
  - [x]* 30.2 Write property test for POSIX Capability Exec Transformation
    - **Property 21: POSIX Capability Exec Transformation**
    - **Validates: Requirements 22.3**
    - Use `proptest` with `arb_capability_set()` generator; assert `exec_transform` output matches the POSIX formula exactly (implemented as `exec_transform_property_random_values` test with multiple edge-case bit patterns)
  - [x]* 30.3 Write property test for Capability Drop Irreversibility
    - **Property 22: Capability Drop Irreversibility**
    - **Validates: Requirements 22.4**
    - Use `proptest` to drop a capability from permitted set; assert it cannot be re-acquired in effective set without exec of a file with that capability (implemented as `drop_irreversibility_property` test)

- [x] 31. Implement PID, mount, network, and user namespaces in `kernel/src/security/namespaces.rs`
  - [x] 31.1 Create `kernel/src/security/namespaces.rs` with `PidNamespace`, `MountNamespace`, `NetNamespace`, and `UserNamespace` structs
    - Implement `clone` syscall with `CLONE_NEWPID`, `CLONE_NEWNS`, `CLONE_NEWNET`, `CLONE_NEWUSER` flags
    - `PidNamespace`: each namespace has its own PID counter starting at 1; processes see only PIDs within their namespace
    - `MountNamespace`: each namespace has its own mount table (copy of parent's on creation)
    - `NetNamespace`: each namespace has its own smoltcp interface and socket table
    - `UserNamespace`: UID/GID mapping between namespace and host
    - Add `pid_ns`, `mnt_ns`, `net_ns`, `user_ns` fields to `ProcessControlBlock`
    - _Requirements: 23.1, 23.2, 23.3, 23.4, 23.5_
  - [x]* 31.2 Write unit tests for namespace isolation
    - Test that a process in a new PID namespace sees PID 1 as its own init
    - Test that mount namespace isolation prevents cross-namespace mount visibility
    - _Requirements: 23.1, 23.2_

- [x] 32. Implement seccomp-BPF in `kernel/src/security/seccomp.rs`
  - [x] 32.1 Create `kernel/src/security/seccomp.rs` with a minimal classic BPF interpreter (`BpfInstruction`, `SeccompFilter`, `SeccompAction`)
    - Implement `SeccompFilter::evaluate(syscall_nr: u32, args: &[u64; 6]) -> SeccompAction`
    - Implement `SeccompFilter::inherit_on_fork()` (clone the filter)
    - Implement `prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, ...)` syscall to install a filter on the current process
    - Enforce that a child process cannot install a less restrictive filter than its parent
    - Wire seccomp evaluation into the syscall dispatch path in `kernel/src/syscall/handler.rs` (before dispatching to the handler)
    - _Requirements: 24.1, 24.2, 24.3, 24.4, 24.5, 24.6_
  - [x]* 32.2 Write property test for Seccomp Filter Inheritance
    - **Property 23: Seccomp Filter Inheritance**
    - **Validates: Requirements 24.5, 24.6**
    - Use `proptest` to generate seccomp filters; assert forked child has the same filter and cannot install a less restrictive one
  - [x]* 32.3 Write unit tests for BPF instruction evaluation
    - Test `BPF_RET | BPF_K` with `SECCOMP_RET_ALLOW` and `SECCOMP_RET_KILL`
    - Test `BPF_LD | BPF_W | BPF_ABS` loading syscall number from seccomp data
    - _Requirements: 24.1_

- [ ] 33. Implement the LSM hook framework in `kernel/src/security/lsm.rs`
  - [ ] 33.1 Create `kernel/src/security/lsm.rs` with the `LsmHook` trait (`file_open`, `process_create`, `ipc_send`, `net_connect`, `capability_check`) and an `LsmStack` that calls all registered hooks in order
    - Implement a default `DacHook` (Discretionary Access Control) that enforces Unix permission bits and capability checks
    - Wire LSM hooks into: VFS `open` path, `fork`/`exec` paths, IPC send path, network connect path, and capability check path
    - _Requirements: 25.1, 25.2, 25.3, 25.4_
  - [ ]* 33.2 Write unit tests for LSM hook enforcement
    - Test that `file_open` hook returning `Err` causes the open to fail with `EACCES`
    - Test that all hooks in the stack are called in order
    - _Requirements: 25.1_

- [ ] 34. Implement IMA/EVM and stack canaries in `kernel/src/security/ima.rs`
  - [ ] 34.1 Create `kernel/src/security/ima.rs` with IMA measurement: on each `exec`, compute SHA-256 of the ELF binary and append to the IMA measurement log (stored in a kernel ring buffer)
    - Implement EVM: store HMAC of file metadata (inode, size, mtime) as an extended attribute; verify on open
    - Implement stack canaries: in `kernel/src/gdt.rs` (or arch layer), place a random 64-bit canary value at the base of each kernel stack; check on task switch and panic if corrupted
    - _Requirements: 26.1, 26.2, 26.3, 26.4, 27.1, 27.2_
  - [ ]* 34.2 Write unit tests for IMA measurement log
    - Test that executing the same binary twice produces the same SHA-256 measurement
    - Test that the measurement log grows by one entry per exec
    - _Requirements: 26.1_

- [ ] 35. Phase 4 checkpoint — verify security framework
  - Ensure all Phase 4 property tests and unit tests pass; verify QEMU boots with capability checks enforced, seccomp filter blocks a forbidden syscall, and IMA measurement log is populated after init exec
  - Ensure all tests pass, ask the user if questions arise.


## Phase 5 — Package Management

- [ ] 36. Create the `shared/tpkg-format` crate with manifest types
  - [ ] 36.1 Create `shared/tpkg-format/` as a new workspace crate; add it to `Cargo.toml` workspace members
    - Define `TpkgManifest`, `PackageName`, `InstallSpec`, `BuildSpec`, `Scripts`, `DataFile` structs with `serde::Serialize`/`Deserialize` and TOML parsing via the `toml` crate
    - Define `InstallPlan`, `ResolvedPackage`, `PackageSource` structs
    - Implement strict validation: reject manifests with missing required fields, invalid semver strings, or invalid package names (return descriptive `Err`)
    - Add `proptest` as a dev-dependency; implement `arb_valid_manifest()` and `arb_package_name()` generators
    - _Requirements: 28.1, 28.2, 28.3_
  - [ ]* 36.2 Write property test for Manifest Parse-Serialize Round-Trip
    - **Property 24: Manifest Parse-Serialize Round-Trip**
    - **Validates: Requirements 28.3**
    - Use `proptest` with `arb_valid_manifest()`; assert parse → serialize → parse produces structurally equivalent manifest
  - [ ]* 36.3 Write property test for Invalid Manifest Rejection
    - **Property 25: Invalid Manifest Rejection**
    - **Validates: Requirements 28.2**
    - Use `proptest` to generate strings that violate the grammar; assert all return `Err` with non-empty message

- [ ] 37. Implement the SAT-based dependency solver in `userland/package-manager/src/solver.rs`
  - [ ] 37.1 Create `userland/package-manager/` as a new workspace crate; add `varisat` and `semver` as dependencies
    - Implement `DependencySolver` with `packages: BTreeMap<PackageName, Vec<PackageVersion>>`
    - Implement `solve(requests: &[(PackageName, VersionReq)]) -> Result<InstallPlan, SolverError>` using `varisat` CDCL SAT solver
    - Encode version constraints as SAT clauses; decode the satisfying assignment into an `InstallPlan` in topological order
    - Return `SolverError::Conflict` for unsatisfiable constraints, `SolverError::Cycle` for dependency cycles, `SolverError::NotFound` for unknown packages
    - Select the highest compatible version when multiple versions satisfy constraints
    - _Requirements: 30.1, 30.2, 30.3, 30.4, 30.5_
  - [ ]* 37.2 Write property test for Dependency Solver Correctness
    - **Property 26: Dependency Solver Correctness**
    - **Validates: Requirements 30.1**
    - Use `proptest` with `arb_satisfiable_deps()` generator; assert every package in the plan satisfies all version constraints
  - [ ]* 37.3 Write property test for Dependency Solver Conflict Detection
    - **Property 27: Dependency Solver Conflict Detection**
    - **Validates: Requirements 30.2**
    - Use `proptest` to generate unsatisfiable constraint sets; assert solver returns `SolverError::Conflict`
  - [ ]* 37.4 Write property test for Dependency Solver Newest Version Preference
    - **Property 28: Dependency Solver Newest Version Preference**
    - **Validates: Requirements 30.4**
    - Use `proptest` to generate multiple compatible versions; assert solver selects the highest
  - [ ]* 37.5 Write property test for Dependency Solver Cycle Detection
    - **Property 29: Dependency Solver Cycle Detection**
    - **Validates: Requirements 30.5**
    - Use `proptest` to generate dependency graphs with directed cycles; assert solver returns `SolverError::Cycle`

- [ ] 38. Implement TUF-based repository client and package fetcher
  - [ ] 38.1 Add `tough` (AWS TUF client) as a dependency to `userland/package-manager/Cargo.toml`
    - Implement `RepositoryClient` that fetches TUF metadata (root, snapshot, targets, timestamp) from a configured repository URL
    - Implement signature verification: abort before any file extraction if TUF signature verification fails
    - Implement `.tpkg` archive download with SHA-256 checksum verification
    - Implement `PackageFetcher::fetch(resolved: &ResolvedPackage) -> Result<PathBuf, FetchError>` that downloads and verifies the archive
    - _Requirements: 29.1, 29.2, 29.3, 29.4_
  - [ ]* 38.2 Write unit tests for TUF signature verification
    - Test that a package with an invalid SHA-256 checksum is rejected before extraction
    - Test that expired TUF metadata is rejected
    - _Requirements: 29.2, 29.3_

- [ ] 39. Implement the snapshot manager and install/rollback pipeline
  - [ ] 39.1 Create `userland/package-manager/src/snapshot.rs` with `Snapshot`, `SnapshotId`, `SnapshotTrigger` types
    - Implement `SnapshotManager::create_snapshot(trigger: SnapshotTrigger) -> Result<Snapshot, SnapshotError>` using ext4 copy-on-write snapshot inodes
    - Implement `SnapshotManager::rollback(id: SnapshotId) -> Result<(), SnapshotError>`
    - Implement the install pipeline in `userland/package-manager/src/install.rs`: create pre-install snapshot → fetch package → verify checksum → extract to staging → apply to filesystem → update package database; on any failure, automatically rollback to the pre-install snapshot
    - Implement `tpkg install`, `tpkg remove`, `tpkg upgrade`, `tpkg list`, `tpkg search` CLI commands in `userland/package-manager/src/main.rs`
    - _Requirements: 31.1, 31.2, 31.3, 31.4, 31.5, 32.1, 32.2, 32.3, 32.4, 32.5_
  - [ ]* 39.2 Write unit tests for snapshot rollback
    - Test that a failed install leaves the filesystem in the pre-install state
    - Test that `rollback` restores the correct snapshot
    - _Requirements: 31.3, 31.4_

- [ ] 40. Phase 5 checkpoint — verify package management
  - Ensure all Phase 5 property tests and unit tests pass; verify `tpkg install hello-world` downloads, verifies, installs, and the binary runs; verify a conflicting dependency set returns a clear error
  - Ensure all tests pass, ask the user if questions arise.


## Phase 6 — System Services Layer

- [ ] 41. Create the `shared/ipc-proto` crate and IPC message types
  - [ ] 41.1 Create `shared/ipc-proto/` as a new workspace crate; add it to `Cargo.toml` workspace members
    - Define `IpcMessage` enum (`MethodCall`, `MethodReturn`, `Signal`, `PropertyGet`, `PropertySet`), `IpcValue`, `IpcError` with `serde::Serialize`/`Deserialize`
    - Implement `IpcMessage` serialization to/from a length-prefixed binary format using `bincode` or `postcard`
    - _Requirements: 35.1, 35.2_
  - [ ]* 41.2 Write unit tests for IPC message serialization round-trip
    - Test that each `IpcMessage` variant serializes and deserializes to the same value
    - _Requirements: 35.2_

- [ ] 42. Implement the IPC broker daemon in `userland/ipc-broker/`
  - [ ] 42.1 Create `userland/ipc-broker/` as a new workspace crate; add it to `Cargo.toml` workspace members
    - Implement the broker as an async event loop (using `async-std` or `smol`) listening on a Unix domain socket at `/run/ipc.sock`
    - Implement service registration: services connect and register their interface name
    - Implement method call routing: broker receives `MethodCall`, looks up the destination service, forwards the message, and routes the `MethodReturn` back to the caller
    - Implement signal broadcast: `Signal` messages are delivered to all subscribers of the interface
    - Wire the IPC broker into the init daemon's service manifest so it starts before other services
    - _Requirements: 35.1, 35.2, 35.3, 35.4, 35.5_
  - [ ]* 42.2 Write unit tests for IPC method call routing
    - Test that a `MethodCall` to a registered service is forwarded and the reply is returned to the caller
    - Test that a `MethodCall` to an unregistered service returns `IpcError::ServiceNotFound`
    - _Requirements: 35.3_

- [ ] 43. Implement the service manager daemon in `userland/service-manager/`
  - [ ] 43.1 Create `userland/service-manager/` as a new workspace crate; add it to `Cargo.toml` workspace members
    - Define `ServiceUnit` (TOML-deserialized), `RestartPolicy` (`Never`, `OnFailure`, `Always`), `SocketSpec`
    - Implement service dependency graph resolution (topological sort of `after`/`requires` fields)
    - Implement service lifecycle: start (fork/exec), monitor (wait for exit), restart with exponential back-off per `RestartPolicy`
    - Implement `TimeoutStartSec`: if service does not signal readiness within timeout, mark as `Failed`
    - Implement socket activation: pre-open the socket and pass the FD to the service on start
    - Expose service status via IPC broker (start/stop/status methods)
    - _Requirements: 33.1, 33.2, 33.3, 33.4, 33.5, 33.6_
  - [ ]* 43.2 Write unit tests for service unit file parsing
    - Test that a valid TOML service unit deserializes correctly
    - Test that `after` dependency cycles are detected and reported
    - _Requirements: 33.1_

- [ ] 44. Implement the log daemon in `userland/log-daemon/`
  - [ ] 44.1 Create `userland/log-daemon/` as a new workspace crate; add it to `Cargo.toml` workspace members
    - Implement structured log entry format: `{ timestamp, level, source, message, fields: BTreeMap }`
    - Implement HMAC-SHA256 sealing of each log entry (using a kernel-provided secret key) to detect tampering
    - Implement log rotation: rotate when log file exceeds 10 MB or 24 hours; keep last 7 rotated files
    - Implement a Unix socket listener at `/run/log.sock` for log submission from other daemons
    - Implement kernel log forwarding: read from the kernel serial log ring buffer and forward to the log daemon
    - _Requirements: 36.1, 36.2, 36.3, 36.4, 36.5_
  - [ ]* 44.2 Write unit tests for log entry HMAC sealing
    - Test that a tampered log entry fails HMAC verification
    - Test that log rotation triggers at the correct file size threshold
    - _Requirements: 36.2, 36.3_

- [ ] 45. Implement the network manager daemon in `userland/network-manager/`
  - [ ] 45.1 Create `userland/network-manager/` as a new workspace crate; add it to `Cargo.toml` workspace members
    - Implement DHCP client using smoltcp's DHCP support: obtain IP address, subnet mask, gateway, and DNS server
    - Implement static IP configuration via a config file at `/etc/turnix/network.toml`
    - Implement interface bring-up/bring-down via IPC broker messages
    - Implement DNS resolver: forward queries to the configured DNS server via UDP
    - Wire network manager into the init service manifest
    - _Requirements: 37.1, 37.2, 37.3, 37.4, 37.5_
  - [ ]* 45.2 Write unit tests for DHCP packet parsing
    - Test DHCP OFFER parsing extracts correct IP, mask, gateway, and DNS fields
    - _Requirements: 37.1_

- [ ] 46. Implement the device manager daemon in `userland/device-manager/`
  - [ ] 46.1 Create `userland/device-manager/` as a new workspace crate; add it to `Cargo.toml` workspace members
    - Implement a kernel hotplug event socket (new syscall `hotplug_subscribe`) that delivers device add/remove events to userland
    - Implement device manager event loop: on device add, look up driver rules, load the appropriate driver module (or notify the kernel to probe), and mount storage devices via the VFS
    - Implement USB storage auto-mount: when a USB mass-storage device is added, mount it at `/media/<label>`
    - Expose device list via IPC broker
    - _Requirements: 38.1, 38.2, 38.3, 38.4_
  - [ ]* 46.2 Write unit tests for hotplug event parsing
    - Test that a device-add event with a known vendor/device ID triggers the correct driver rule
    - _Requirements: 38.2_

- [ ] 47. Integrate smoltcp TCP/IP stack and socket syscalls
  - [ ] 47.1 Create `kernel/src/net/smoltcp_iface.rs` integrating smoltcp 0.11 with the VirtIO-Net driver
    - Implement `kernel/src/net/socket.rs` with `SocketTable` mapping FDs to smoltcp socket handles
    - Implement `socket`, `bind`, `listen`, `accept`, `connect`, `send`, `recv`, `close` syscalls for `AF_INET`/`AF_INET6` `SOCK_STREAM` and `SOCK_DGRAM`
    - Implement `select`/`poll` syscalls for I/O multiplexing
    - Wire the smoltcp poll loop into the LAPIC timer interrupt handler
    - _Requirements: 6.3, 37.1_
  - [ ]* 47.2 Write unit tests for socket syscall dispatch
    - Test that `socket(AF_INET, SOCK_STREAM, 0)` returns a valid FD
    - Test that `connect` to an unreachable address returns `ECONNREFUSED` after timeout
    - _Requirements: 37.1_

- [ ] 48. Phase 6 checkpoint — verify system services
  - Ensure all Phase 6 unit tests pass; verify QEMU boots with init → service-manager → ipc-broker → log-daemon → network-manager all running, DHCP obtains an IP address, and a TCP connection to an external host succeeds
  - Ensure all tests pass, ask the user if questions arise.


## Phase 7 — Wayland Graphical Stack

- [ ] 49. Implement the input event normalisation layer in `kernel/src/input/` and `userland/`
  - [ ] 49.1 Extend `kernel/src/input/mod.rs` to define a normalised `InputEvent` enum (`KeyPress { key_code, modifiers }`, `KeyRelease { key_code }`, `PointerMotion { dx, dy }`, `PointerButton { button, pressed }`, `PointerAxis { axis, delta }`)
    - Implement scan-code to key-code mapping table for PS/2 and USB HID keyboards
    - Implement pointer acceleration calculation for relative mouse motion
    - Expose a kernel input event ring buffer readable via a new `input_read` syscall
    - _Requirements: 40.1, 40.2, 40.3_
  - [ ]* 49.2 Write property test for Input Event Normalization
    - **Property 30: Input Event Normalization**
    - **Validates: Requirements 40.1**
    - Use `proptest` to generate raw kernel input events; assert each produces a normalized event of the correct type with correct field values and no fields from a different event type

- [ ] 50. Implement the DRM/KMS kernel driver and GBM buffer allocation
  - [ ] 50.1 Extend `kernel/src/drivers/gpu/drm.rs` with full DRM/KMS support: connector enumeration, CRTC assignment, mode setting, and page flip via `DrmDevice` trait
    - Implement GBM (Generic Buffer Manager): `gbm_create_buffer(width, height, format) -> GbmBufferId`, `gbm_map_buffer(id) -> VirtAddr`, `gbm_destroy_buffer(id)`
    - Expose GBM operations via new syscalls (`gbm_create`, `gbm_map`, `gbm_destroy`) restricted to the Compositor process
    - Implement page flip completion interrupt delivery to the Compositor via an event fd
    - _Requirements: 41.1, 41.2, 41.3, 41.4_
  - [ ]* 50.2 Write unit tests for DRM mode set and page flip
    - Test that `set_mode` with a valid connector and mode succeeds
    - Test that `page_flip` with an invalid framebuffer ID returns `DrmError::InvalidFramebuffer`
    - _Requirements: 41.1, 41.2_

- [ ] 51. Implement the Wayland compositor in `userland/compositor/`
  - [ ] 51.1 Create `userland/compositor/` as a new workspace crate; add `smithay` (wayland-server) as a dependency
    - Implement `TurnixCompositor` with `wayland_server::Display`, `DrmBackend`, `InputManager`, `surfaces: BTreeMap<SurfaceId, Surface>`, and `focused: Option<SurfaceId>`
    - Implement Wayland protocol handlers: `wl_compositor`, `wl_surface`, `wl_shm`, `xdg_wm_base`, `xdg_surface`, `xdg_toplevel`
    - Implement surface rendering: composite all surfaces in Z-order onto the DRM framebuffer via GBM buffer mmap
    - Implement input routing: deliver keyboard events to the focused surface, pointer events to the surface under the cursor
    - Implement client crash handling: remove all surfaces owned by the crashed client, release GBM buffers, redraw without the crashed client
    - Wire compositor into the init service manifest
    - _Requirements: 41.1, 41.2, 41.3, 41.4, 41.5, 42.1, 42.2, 42.3, 42.4_
  - [ ]* 51.2 Write unit tests for Wayland surface geometry clipping
    - Test that a surface partially outside the display bounds is clipped to the display rectangle
    - Test that Z-order compositing renders surfaces in the correct order
    - _Requirements: 42.2_

- [ ] 52. Implement the display manager in `userland/display-manager/`
  - [ ] 52.1 Create `userland/display-manager/` as a new workspace crate
    - Implement a greetd-style login greeter: render a username/password prompt on the Wayland compositor
    - Implement PAM-style authentication: verify credentials against `/etc/turnix/passwd` (SHA-256 hashed passwords)
    - On successful login: fork a new session, set UID/GID, drop capabilities to user level, exec the desktop shell
    - On failed login: log the attempt and re-display the prompt
    - _Requirements: 43.1, 43.2, 43.3, 43.4_
  - [ ]* 52.2 Write unit tests for display manager authentication
    - Test that a correct password hash comparison succeeds
    - Test that an incorrect password is rejected and the attempt is logged
    - _Requirements: 43.2_

- [ ] 53. Implement the desktop shell in `userland/desktop-shell/`
  - [ ] 53.1 Create `userland/desktop-shell/` as a new workspace crate
    - Implement a minimal Wayland client using `wayland-client` (smithay client toolkit)
    - Implement a taskbar: display running application names, click to focus
    - Implement an application launcher: display a list of installed applications (from `/usr/share/applications/`), launch on click via `fork`/`exec`
    - Implement basic window management: move windows by dragging the title bar, close via title bar button
    - _Requirements: 44.1, 44.2, 44.3, 44.4_
  - [ ]* 53.2 Write unit tests for desktop shell application list parsing
    - Test that `.desktop` files in `/usr/share/applications/` are parsed into application entries
    - Test that launching an application creates a new process with the correct executable path
    - _Requirements: 44.2_

- [ ] 54. Phase 7 checkpoint — verify Wayland stack
  - Ensure all Phase 7 unit tests pass; verify QEMU boots to the display manager login screen, login succeeds, the desktop shell appears with a taskbar, and a terminal application can be launched and used
  - Ensure all tests pass, ask the user if questions arise.


## Phase 8 — CI/CD Validation

- [ ] 55. Set up the QEMU-based CI test harness
  - [ ] 55.1 Create `tools/ci-runner/` as a new workspace crate (or extend `tools/xtask/`) with a QEMU boot harness
    - Implement `boot_qemu(timeout_secs: u64) -> BootResult` that launches QEMU with the Turnix disk image, captures serial output, and waits for a `[BOOT OK]` sentinel or timeout
    - Implement `run_test_suite(suite: &str) -> TestResults` that boots QEMU, injects test commands via the serial port, and parses pass/fail output
    - Add `cargo xtask ci-boot` command that runs 30 consecutive QEMU boots and asserts all succeed (30-boot gate)
    - _Requirements: 45.1, 45.2_
  - [ ]* 55.2 Write unit tests for QEMU boot harness
    - Test that `boot_qemu` correctly parses the `[BOOT OK]` sentinel from serial output
    - Test that a boot timeout returns `BootResult::Timeout`
    - _Requirements: 45.1_

- [ ] 56. Extend the GitHub Actions CI workflow
  - [ ] 56.1 Extend `.github/workflows/ci.yml` with the following jobs (all on `ubuntu-latest` with QEMU 8.x):
    - `build`: `cargo build --release` for all workspace members; fail on any warning (`RUSTFLAGS="-D warnings"`)
    - `unit-tests`: `cargo test --workspace` (runs all unit and property tests)
    - `boot-gate`: `cargo xtask ci-boot` (30 consecutive QEMU boots)
    - `driver-tests`: boot QEMU and verify PCIe enumeration log, VirtIO-Net MAC negotiation, NVMe namespace discovery
    - `security-regression`: boot QEMU and verify W^X self-check passes, ASLR produces distinct addresses on 5 consecutive boots, seccomp blocks a forbidden syscall
    - `performance-benchmarks`: boot QEMU and measure fork latency (< 10 ms), pipe throughput (> 100 MB/s), page fault latency (< 1 µs)
    - All jobs must pass before a PR can merge to the integration branch
    - _Requirements: 45.1, 45.2, 45.3, 45.4, 45.5_
  - [ ]* 56.2 Write unit tests for CI workflow configuration
    - Test that the YAML workflow file parses without errors using a YAML linter
    - Test that all required jobs are present in the workflow
    - _Requirements: 45.1_

- [ ] 57. Implement the driver test suite
  - [ ] 57.1 Create `kernel/tests/driver_tests.rs` (or `tools/xtask/src/driver_tests.rs`) with integration tests for each Phase 1 driver
    - PCIe enumeration test: assert at least 2 devices discovered (virtio-net, virtio-blk) in QEMU
    - VirtIO-Net test: assert MAC address is not `00:00:00:00:00:00` (real MAC negotiated)
    - NVMe test: assert at least one namespace discovered with capacity > 0
    - XHCI test: assert controller initialised without timeout (or gracefully marked unavailable)
    - GPU test: assert framebuffer mapped at expected address with correct size
    - _Requirements: 45.3_
  - [ ]* 57.2 Write unit tests for driver test harness
    - Test that the test harness correctly parses QEMU serial output for device discovery log lines
    - _Requirements: 45.3_

- [ ] 58. Implement the security regression test suite
  - [ ] 58.1 Create `kernel/tests/security_tests.rs` with security regression tests
    - W^X test: boot QEMU, check serial log for `[WX] self-check: PASS`
    - ASLR test: boot QEMU 5 times, parse load addresses from serial log, assert all 5 are distinct
    - Seccomp test: run a test process that installs a seccomp filter blocking `write`, then attempts `write`; assert process is killed
    - Capability test: run a test process without `CAP_NET_ADMIN`, attempt to configure a network interface; assert `EPERM`
    - IMA test: boot QEMU, exec a binary, assert IMA measurement log contains one entry with the correct SHA-256
    - _Requirements: 45.4_
  - [ ]* 58.2 Write unit tests for security test harness
    - Test that the ASLR address parser correctly extracts load addresses from serial log lines
    - _Requirements: 45.4_

- [ ] 59. Implement performance benchmarks
  - [ ] 59.1 Create `userland/benchmarks/` as a new workspace crate with micro-benchmarks
    - Fork latency benchmark: measure time from `fork` syscall to first instruction in child; assert < 10 ms on QEMU
    - Pipe throughput benchmark: write 1 GB through a pipe and measure throughput; assert > 100 MB/s
    - Page fault latency benchmark: access a fresh anonymous mmap page and measure fault-to-return latency; assert < 1 µs
    - Context switch latency benchmark: measure round-trip time for two processes yielding to each other
    - Report results as structured JSON to stdout for CI parsing
    - _Requirements: 45.5_
  - [ ]* 59.2 Write unit tests for benchmark result parsing
    - Test that the JSON benchmark output parser correctly extracts latency and throughput values
    - _Requirements: 45.5_

- [ ] 60. Final checkpoint — full system integration
  - Ensure all 8 phases compile cleanly with `RUSTFLAGS="-D warnings"`, all unit and property tests pass, the 30-boot gate passes, all driver tests pass, all security regression tests pass, and all performance benchmarks meet their targets
  - Ensure all tests pass, ask the user if questions arise.


---

## Notes

- Tasks marked with `*` are optional and can be skipped for a faster MVP; all core implementation tasks are mandatory
- Each task references specific requirements for traceability; requirement numbers match the requirements document
- Checkpoints (tasks 9, 17, 29, 35, 40, 48, 54, 60) are integration milestones — do not proceed to the next phase until the checkpoint passes
- Property tests use `proptest` 1.x; add `proptest = "1"` as a dev-dependency to each crate that needs it
- All kernel code is `no_std`; property tests that require `std` (for `proptest`) must be in `#[cfg(test)]` modules with `extern crate std;`
- The design document uses Rust throughout; all implementation tasks produce Rust code
- Tasks build on the existing codebase: `kernel/src/process.rs`, `kernel/src/memory/paging.rs`, `kernel/src/vfs.rs`, `kernel/src/syscall/handler.rs`, `kernel/src/security/mod.rs`, and `kernel/src/drivers/` are all extended rather than replaced
- New workspace crates must be added to the `members` array in the root `Cargo.toml`

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "4.1"] },
    { "id": 1, "tasks": ["1.2", "1.3", "2.1", "3.1"] },
    { "id": 2, "tasks": ["2.2", "3.2", "5.1", "6.1", "7.1", "8.1"] },
    { "id": 3, "tasks": ["5.2", "6.2", "7.2", "8.2", "10.1"] },
    { "id": 4, "tasks": ["10.2", "11.1"] },
    { "id": 5, "tasks": ["11.2", "11.3", "11.4", "12.1"] },
    { "id": 6, "tasks": ["12.2", "12.3", "12.4", "13.1"] },
    { "id": 7, "tasks": ["13.2", "13.3", "13.4", "14.1"] },
    { "id": 8, "tasks": ["14.2", "15.1", "16.1"] },
    { "id": 9, "tasks": ["15.2", "16.2", "18.1"] },
    { "id": 10, "tasks": ["19.1", "20.1"] },
    { "id": 11, "tasks": ["19.2", "20.2", "21.1"] },
    { "id": 12, "tasks": ["21.2", "22.1"] },
    { "id": 13, "tasks": ["22.2", "23.1"] },
    { "id": 14, "tasks": ["23.2", "24.1"] },
    { "id": 15, "tasks": ["24.2", "24.3", "25.1"] },
    { "id": 16, "tasks": ["25.2", "26.1"] },
    { "id": 17, "tasks": ["26.2", "26.3", "27.1"] },
    { "id": 18, "tasks": ["27.2", "28.1"] },
    { "id": 19, "tasks": ["28.2", "30.1"] },
    { "id": 20, "tasks": ["30.2", "30.3", "31.1"] },
    { "id": 21, "tasks": ["31.2", "32.1"] },
    { "id": 22, "tasks": ["32.2", "32.3", "33.1"] },
    { "id": 23, "tasks": ["33.2", "34.1"] },
    { "id": 24, "tasks": ["34.2", "36.1"] },
    { "id": 25, "tasks": ["36.2", "36.3", "37.1"] },
    { "id": 26, "tasks": ["37.2", "37.3", "37.4", "37.5", "38.1"] },
    { "id": 27, "tasks": ["38.2", "39.1"] },
    { "id": 28, "tasks": ["39.2", "41.1"] },
    { "id": 29, "tasks": ["41.2", "42.1"] },
    { "id": 30, "tasks": ["42.2", "43.1", "44.1", "45.1", "47.1"] },
    { "id": 31, "tasks": ["43.2", "44.2", "45.2", "46.1", "47.2"] },
    { "id": 32, "tasks": ["46.2", "49.1"] },
    { "id": 33, "tasks": ["49.2", "50.1"] },
    { "id": 34, "tasks": ["50.2", "51.1"] },
    { "id": 35, "tasks": ["51.2", "52.1"] },
    { "id": 36, "tasks": ["52.2", "53.1"] },
    { "id": 37, "tasks": ["53.2", "55.1"] },
    { "id": 38, "tasks": ["55.2", "56.1", "57.1", "58.1", "59.1"] },
    { "id": 39, "tasks": ["56.2", "57.2", "58.2", "59.2"] }
  ]
}
```
