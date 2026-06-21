# Turnix OS — Architectural Gap Fixes

> **Purpose:** This document tracks all remaining incomplete implementations that require significant new subsystems to fix. Each entry describes what exists, what's needed, and the implementation path.
>
> **Last updated:** 2026-06-21
>
> **Status legend:** NOT STARTED | IN PROGRESS | BLOCKED | DONE

---

## Priority Tiers

| Tier | Description | Target |
|------|-------------|--------|
| **P0 — Critical** | System cannot ship without these | Before v1.0 |
| **P1 — High** | Significant functionality gap | v1.0 – v1.1 |
| **P2 — Medium** | Important but not blocking | v1.2+ |
| **P3 — Low** | Cleanup / nice-to-have | Future |

---

## 1. Filesystem & Storage

### GFS-1: Block Device I/O Layer
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P0 — Critical |
| **Effort** | XL (4–6 weeks) |
| **Files** | `kernel/src/drivers/nvme.rs`, new `kernel/src/block/` |
| **Depends on** | NVMe driver (exists, basic) |

**What exists:**
- NVMe driver with basic single-page I/O and PRP list structure
- `BYTES_PER_PRP` constant defined but unused
- Queue pair setup and submission/completion ring basics

**What's needed:**
- Generic block device abstraction (`BlockDevice` trait with `read_sectors`, `write_sectors`, `flush`)
- Multi-page PRP list management for large I/O
- DMA-safe buffer pool with cache-line alignment
- I/O completion polling and interrupt-driven paths
- Block layer request queue with merging and reordering
- Write-back cache with dirty page tracking

**Implementation plan:**
1. Define `BlockDevice` trait in `kernel/src/block/mod.rs`
2. Implement NVMe multi-page I/O (PRP list builder)
3. Add DMA buffer allocator (page-aligned, non-cacheable)
4. Wire block device into ext4 backend (replace tmpfs delegation)

---

### GFS-2: ext4 On-Disk Backend
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P0 — Critical |
| **Effort** | XL (6–8 weeks) |
| **Files** | `kernel/src/fs/ext4.rs`, new `kernel/src/fs/ext4/` |
| **Depends on** | GFS-1 |

**What exists:**
- `Ext4Backend` struct that delegates entirely to `TmpfsBackend`
- `FsBackend` trait implementation (read, write, mkdir, lookup, etc.)
- Module doc clearly states "in-memory only"

**What's needed:**
- On-disk format parser: superblock, block group descriptors, inode table, block bitmaps
- Extent tree or indirect block traversal for large files
- Inode allocation and deallocation
- Block allocator (free space management)
- Directory indexing (htree for large directories)
- Journal (ext3-style) for crash consistency
- Write-back dirty page tracking and flush

**Implementation plan:**
1. Parse ext4 superblock and block group descriptors from NVMe
2. Implement block bitmap allocator
3. Implement inode table reader/writer
4. Implement extent tree parser
5. Add journal replay for crash recovery
6. Replace TmpfsBackend delegation with real I/O calls

---

### GFS-3: ext2 Write Support
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P1 — High |
| **Effort** | XL (4–6 weeks) |
| **Files** | `kernel/src/fs/ext2.rs` |
| **Depends on** | GFS-1 |

**What exists:**
- ext2 read-only implementation with inode parsing, directory traversal
- All write operations return `FsError::PermissionDenied`
- `sync()` is a no-op

**What's needed:**
- Block allocator (block bitmap manipulation)
- Inode allocator (inode bitmap manipulation)
- Write path: data block allocation, indirect block management
- Directory modification: add/remove entries, inode link count
- Journal for crash consistency
- `sync()` that flushes dirty blocks to NVMe

---

### GFS-4: VFS Extended Attributes (xattr)
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P1 — High |
| **Effort** | L (2–3 weeks) |
| **Files** | `kernel/src/fs/vfs.rs`, backend files |
| **Depends on** | GFS-2 or GFS-3 |

**What exists:**
- VFS trait has no xattr methods
- EVM and FileCaps note they need xattr support

**What's needed:**
- `xattr_get`, `xattr_set`, `xattr_list`, `xattr_remove` on `FsBackend` trait
- On-disk xattr storage (ext4 inline xattr or separate block)
- Security namespace for SELinux-style labels
- `security.` namespace for EVM HMAC storage
- `capability.` namespace for FileCaps

---

### GFS-5: Per-Process Working Directory
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P2 — Medium |
| **Effort** | S (hours) |
| **Files** | `kernel/src/fs/vfs.rs`, `kernel/src/syscall/handler.rs` |

**What exists:**
- `Vfs` struct has a `cwd: String` field marked `#[allow(dead_code)]`
- Set to "/" and never updated

**What's needed:**
- Implement `chdir` syscall to update `cwd`
- Use `cwd` in path resolution for relative paths
- Per-process `cwd` (currently it's global to Vfs)

---

### GFS-6: Remove NullBackend Legacy Path
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P2 — Medium |
| **Effort** | M (1–2 days) |
| **Files** | `kernel/src/fs/vfs.rs` (lines 1368–1409) |

**What exists:**
- `NullBackend` struct that silently discards all reads/writes
- Used for legacy flat-entry file descriptors

**What's needed:**
- Migrate all legacy FD users to path-based VFS
- Remove `NullBackend` and the flat-entry compatibility layer
- Return proper errors for invalid FDs

---

## 2. Networking

### GNET-1: DHCP Client Protocol
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P0 — Critical |
| **Effort** | L (2–3 weeks) |
| **Files** | `userland/network-manager/src/dhcp.rs` (new), `kernel/src/net/` |
| **Depends on** | UDP socket (exists, basic) |

**What exists:**
- UDP socket support in kernel (bind, send, recv)
- `DhcpLease` struct with IP, gateway, mask, DNS
- `run_dhcp()` returns hardcoded lease

**What's needed:**
- DHCP DISCOVER packet construction (broadcast to 255.255.255.255:67)
- OFFER parsing and REQUEST generation
- ACK handling and lease file writing
- Lease renewal timer (T1/T2)
- UDP socket integration for real network I/O
- Broadcast/multicast socket support

**Implementation plan:**
1. Define DHCP packet structures (BOOTP format)
2. Implement DISCOVER/OFFER/REQUEST/ACK state machine
3. Integrate with UDP socket for packet send/recv
4. Add lease management (T1/T2 renewal timers)
5. Write lease to `/var/run/dhcp.lease`

---

### GNET-2: Network Interface Configuration
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P0 — Critical |
| **Effort** | L (2–3 weeks) |
| **Files** | new `kernel/src/net/ifconfig.rs`, `userland/network-manager/src/` |
| **Depends on** | Virtio-net driver (exists) |

**What exists:**
- `LinuxInterfaceController` that prints to stderr and returns `Ok(())`
- Single hardcoded "eth0" interface
- Virtio-net driver with basic TX/RX

**What's needed:**
- Kernel syscall for interface configuration (set IP, netmask, gateway, MTU)
- Interface state management (UP/DOWN/running flags)
- ARP table management
- Routing table (default route, subnet routes)
- Netlink-like interface for userland query/control

**Implementation plan:**
1. Add `ioctl` or dedicated syscalls for interface configuration
2. Implement IP address assignment on virtio-net
3. Add routing table (in-kernel or userland)
4. Implement ARP for local network resolution

---

### GNET-3: TCP/UDP Connection Improvements
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P1 — High |
| **Effort** | M (3–5 days) |
| **Files** | `kernel/src/net/socket.rs` |

**What exists:**
- Basic TCP connect/listen/accept/bind
- UDP bind/send/recv
- `sys_accept()` reuses same FD (POSIX violation)

**What's needed:**
- `accept()` must create a NEW socket FD (keep listening socket open)
- TCP backlog queue with SYN cookie protection
- UDP connected-socket semantics (set default remote endpoint)
- Socket option support (SO_REUSEADDR, SO_KEEPALIVE, etc.)
- Non-blocking socket support (O_NONBLOCK)

---

### GNET-4: IPC Broker Authentication
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P1 — High |
| **Effort** | L (1–2 weeks) |
| **Files** | `userland/ipc-broker/src/lib.rs` |

**What exists:**
- Broker accepts any Unix socket connection
- No authentication, no ACLs, no encryption

**What's needed:**
- Service authentication (credential exchange on connect)
- Per-interface ACLs (which services can call which methods)
- Audit logging of IPC calls
- Optional TLS for sensitive channels

---

## 3. Security & Authentication

### GSEC-1: VFS Extended Attribute Support
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P0 — Critical |
| **Effort** | L (2–3 weeks) |
| **Files** | `kernel/src/fs/vfs.rs`, backend files |
| **Depends on** | GFS-2 (ext4) or GFS-3 (ext2) |

**What exists:**
- EVM has `evm_verify()` and `evm_compute_hmac()` but no xattr storage
- FileCaps noted as TODO when xattr available

**What's needed:**
- xattr methods on `FsBackend` trait
- On-disk xattr storage format
- Security namespace for EVM and MAC labels
- Integration into file open/create paths

---

### GSEC-2: EVM VFS Integration
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P1 — High |
| **Effort** | L (2–3 weeks) |
| **Files** | `kernel/src/security/ima.rs`, `kernel/src/fs/vfs.rs` |
| **Depends on** | GSEC-1 |

**What exists:**
- `evm_verify()` with HMAC-SHA256 + constant-time comparison
- Hardcoded HMAC key (`turnix-evm-hmac-key-2024-v1!1234`)
- Module header says "stub — wired once VFS xattr support is available"

**What's needed:**
- Store EVM HMAC in `security.evm` xattr on each file
- Verify HMAC on file open (integrity check)
- Re-compute HMAC on file metadata change
- TPM-derived key (replace hardcoded constant)
- Policy engine (which files require EVM verification)

---

### GSEC-3: FileCaps xattr
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P2 — Medium |
| **Effort** | L (1–2 weeks) |
| **Files** | `kernel/src/process.rs` (line 715), `kernel/src/security/capabilities.rs` |
| **Depends on** | GSEC-1 |

**What exists:**
- `exec_transform()` uses process capability set
- TODO comment: "when VFS xattr is available, read FileCaps"
- `Capability` enum with from_bit/to_bit conversion

**What's needed:**
- Read `security.capability` xattr from ELF binary at exec
- Parse file capability set (permitted, effective, inheritable)
- Apply capability transformation on exec
- `setcap` / `getcap` userland tools

---

### GSEC-4: MAC (Mandatory Access Control) Framework
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P1 — High |
| **Effort** | XL (4–6 weeks) |
| **Files** | `kernel/src/security/lsm.rs`, new `kernel/src/security/mac/` |
| **Depends on** | GSEC-1 |

**What exists:**
- `LsmStack` with `LsmHook` trait (file_open, process_create, ipc_send, net_connect, capability_check)
- `DacHook` implementing basic DAC checks
- `LsmError::AccessDenied` error type

**What's needed:**
- Security label assignment (process and file contexts)
- MAC policy engine (label-based access decisions)
- Policy language (simple rule format)
- Integration with xattr for file labels
- Process label inheritance on fork/exec
- Network label checks (socket connect/send)
- Audit trail for denials

**Implementation plan:**
1. Define `MacHook` trait extending `LsmHook`
2. Implement `SecurityLabel` type (user:role:level format)
3. Add label storage to process context and file xattrs
4. Implement policy parser (allow/deny rules)
5. Wire into LSM hook calls
6. Add `setfattr` / `getfattr` userland tools

---

### GSEC-5: TPM Key Storage
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P1 — High |
| **Effort** | L (2–3 weeks) |
| **Files** | new `kernel/src/drivers/tpm.rs` |

**What exists:**
- Hardcoded EVM HMAC key in source
- No TPM driver

**What's needed:**
- TPM 2.0 driver (TIS interface for QEMU)
- Key derivation from TPM-stored seed
- Secure key storage for EVM, disk encryption, IPC
- `tpm2-seal` / `tpm2-unseal` operations

---

## 4. GPU & Display

### GGPU-1: VirtIO-GPU Virtqueue Protocol
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P1 — High |
| **Effort** | XL (4–6 weeks) |
| **Files** | `kernel/src/drivers/gpu/drm.rs`, new `kernel/src/drivers/gpu/virtio_gpu.rs` |
| **Depends on** | VirtIO transport (exists) |

**What exists:**
- `VirtioGpuDriver` struct with stub `DrmDevice` impl
- Returns hardcoded 1920×1080 from UEFI framebuffer
- `set_mode()`, `create_framebuffer()`, `page_flip()` are no-ops

**What's needed:**
- VirtIO-GPU control queue (command/event virtqueue pair)
- Display info query (VIRTIO_GPU_CMD_GET_DISPLAY_INFO)
- Scanout setup (VIRTIO_GPU_CMD_SET_SCANOUT)
- 2D resource creation (VIRTIO_GPU_CMD_RESOURCE_CREATE_2D)
- Host transfer (VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D)
- Resource attachment (VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING)
- Flush (VIRTIO_GPU_CMD_RESOURCE_FLUSH)

**Implementation plan:**
1. Implement VirtIO-GPU control queue (virtqueue allocate, notify)
2. Add GET_DISPLAY_INFO command to query modes
3. Implement SET_SCANOUT for framebuffer assignment
4. Add RESOURCE_CREATE_2D + ATTACH_BACKING for buffer allocation
5. Implement TRANSFER_TO_HOST + FLUSH for updates

---

### GGPU-2: Display Mode Discovery (EDID)
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P1 — High |
| **Effort** | L (2–3 weeks) |
| **Files** | new `kernel/src/drivers/gpu/edid.rs`, `kernel/src/drivers/gpu/drm.rs` |
| **Depends on** | GGPU-1 |

**What exists:**
- `SCREEN_W` / `SCREEN_H` constants hardcoded to 1920×1080
- Used in 30+ locations across kernel and userland

**What's needed:**
- EDID 1.3 parser (read from display hardware or VBIOS)
- Display mode list (resolutions, refresh rates, timings)
- Mode selection logic (preferred mode, fallback)
- Runtime resolution change API
- Replace all `SCREEN_W` / `SCREEN_H` constants with runtime queries

---

### GGPU-3: Double-Buffering & VBlank
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P1 — High |
| **Effort** | L (2–3 weeks) |
| **Files** | `kernel/src/drivers/gpu/drm.rs`, `userland/compositor/` |
| **Depends on** | GGPU-1 |

**What exists:**
- `page_flip()` is a no-op
- Single-buffered rendering (no tear-free)
- `flip_complete` tracking added but not functional

**What's needed:**
- Front/back buffer management
- VBlank interrupt handling (drm_vblank_wait)
- Atomic page flip (non-blocking)
- Fence/sync for GPU-CPU synchronization
- Compositor integration (Wayland frame callback)

---

## 5. Architecture Ports

### GARCH-1: AArch64 Exception Model
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P0 — Critical (if AArch64 target) |
| **Effort** | XL (6–8 weeks) |
| **Files** | `kernel/src/arch/aarch64/gdt.rs`, `kernel/src/arch/aarch64/interrupts/` |

**What exists:**
- `gdt::init()`, `gdt::init_for_cpu()`, `gdt::set_interrupt_stack()`, `gdt::reload_gdt()` — all empty stubs
- `interrupts::init()`, `interrupts::enable_interrupts()`, `interrupts::disable_interrupts()`, `interrupts::halt()` — all empty stubs
- `apic::init_per_cpu()`, `apic::signal_eoi()` — empty stubs

**What's needed:**
- VBAR_EL1 setup with exception vector table
- Per-CPU exception stack allocation
- ESR_EL1 (Exception Syndrome Register) decoding
- FAR_EL1 (Fault Address Register) for data aborts
- DAIF register manipulation for interrupt masking

---

### GARCH-2: AArch64 Syscall Dispatch
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P0 — Critical (if AArch64 target) |
| **Effort** | XL (4–6 weeks) |
| **Files** | `kernel/src/arch/aarch64/syscall_arch.rs` |

**What exists:**
- `syscall_dispatch()` hangs in infinite `spin_loop()`
- No SVC vector setup

**What's needed:**
- SVC exception vector in VBAR_EL1
- Register decoding (x8 = syscall number, x0–x5 = args)
- Return value in x0
- ERET for return to userspace
- Syscall table dispatch (same handler as x86_64)

---

### GARCH-3: AArch64 GIC Driver
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P0 — Critical (if AArch64 target) |
| **Effort** | XL (4–6 weeks) |
| **Files** | new `kernel/src/drivers/gic/` |

**What exists:**
- Empty `apic::init_per_cpu()` and `apic::signal_eoi()`
- No GIC MMIO register definitions

**What's needed:**
- GICv2/v3 distributor initialization (GICD_*)
- Redistributor initialization (GICC_*, GICR_*)
- Interrupt priority and target CPU configuration
- EOI (End of Interrupt) handling
- Per-CPU interrupt routing
- SGI (Software Generated Interrupt) for IPI

---

### GARCH-4: Serial MMIO for Non-x86
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P1 — High |
| **Effort** | M (3–5 days) |
| **Files** | `shared/serial/src/lib.rs` |

**What exists:**
- `out8()` / `in8()` are no-ops on non-x86
- No serial output on AArch64/RISC-V

**What's needed:**
- PL011 UART driver for AArch64 (MMIO register access)
- NS16550 driver for RISC-V (if target)
- `#[cfg(target_arch)]` dispatch in serial crate
- Early boot console for kernel messages

---

## 6. Device Management

### GDEV-1: XHCI Extended Features
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P2 — Medium |
| **Effort** | L (2–3 weeks) |
| **Files** | `kernel/src/drivers/xhci.rs` |

**What exists:**
- Basic XHCI probe and init
- 30+ `#[allow(dead_code)]` register constants
- Port status extraction helpers

**What's needed:**
- Extended capability parsing (USB legacy support, etc.)
- Device context management (input/output contexts)
- Transfer ring management (TRB ring)
- Isochronous transfer support
- USB device enumeration (GET_DESCRIPTOR)
- HID driver (keyboard/mouse)
- Mass storage protocol (BBB/CBI)

---

### GDEV-2: NVMe Multi-Page I/O
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P1 — High |
| **Effort** | M (3–5 days) |
| **Files** | `kernel/src/drivers/nvme.rs` |

**What exists:**
- Basic single-page NVMe I/O
- `BYTES_PER_PRP` constant defined but unused
- Queue pair setup

**What's needed:**
- Multi-page PRP list builder for large transfers
- Physical address scatter-gather
- I/O with more than 4KB payload
- Flush command support
- Write zeros command

---

### GDEV-3: Virtio-Net Robust Error Handling
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P2 — Medium |
| **Effort** | M (3–5 days) |
| **Files** | `kernel/src/drivers/virtio_net.rs` |

**What exists:**
- Basic TX/RX path
- Multiple `#[allow(dead_code)]` error types and helpers

**What's needed:**
- Device reset/recovery on error
- Buffer chain handling for large packets
- Multicast filter configuration
- VLAN tag support
- Statistics counters

---

## 7. Userland Services

### GUSR-1: Service Manager Process Supervision
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P1 — High |
| **Effort** | L (2–3 weeks) |
| **Files** | `userland/service-manager/src/lib.rs` |

**What exists:**
- State tracking (`mark_running`, `mark_stopped`, `mark_failed`)
- Service manifest parsing
- No actual process spawning or monitoring

**What's needed:**
- Process spawning via `fork`/`exec`
- Child process monitoring (waitpid integration)
- Restart policy (always, on-failure, never)
- Watchdog (health check pings)
- Resource limits (CPU, memory, file descriptors)
- Graceful shutdown (SIGTERM → SIGKILL timeout)

---

### GUSR-2: Kernel Log Forwarding
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P2 — Medium |
| **Effort** | M (3–5 days) |
| **Files** | `kernel/src/serial.rs`, `userland/log-daemon/src/lib.rs` |

**What exists:**
- `KernelLogSource` trait with `poll()` method
- `FileKernelLogSource` that reads from `/var/log/kernel.log`
- No actual kernel-to-userland log transport

**What's needed:**
- Kernel ring buffer in shared memory (mmap'd page)
- `dmesg`-style syscall to query/clear the ring buffer
- Log daemon reading from shared memory
- Log level filtering (debug, info, warn, error)
- Log rate limiting to prevent log flooding

---

### GUSR-3: Package Manager Network Fetcher
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P2 — Medium |
| **Effort** | L (2–3 weeks) |
| **Files** | `userland/package-manager/src/fetcher.rs` |

**What exists:**
- `FetchError` type and `verify_sha256()` function
- Actual fetch returns `Ok(())` (no network I/O)

**What's needed:**
- HTTPS client (TLS 1.3 over TCP)
- TUF (The Update Framework) client
- Repository metadata parsing
- Package download with resume support
- Signature verification (Ed25519)

---

### GUSR-4: Dynamic Desktop Resolution
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P2 — Medium |
| **Effort** | S (hours) |
| **Files** | `userland/desktop-shell/src/main.rs` (lines 21–22) |
| **Depends on** | GGPU-2 |

**What exists:**
- `SCREEN_W: u32 = 1920` and `SCREEN_H: u32 = 1080` compile-time constants

**What's needed:**
- Query compositor or kernel for actual display mode
- Store in runtime variables
- Re-layout UI on resolution change

---

## 8. Cross-Cutting Concerns

### GCUT-1: Hardcoded EVM HMAC Key
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P0 — Critical |
| **Effort** | L (2–3 weeks) |
| **Files** | `kernel/src/security/ima.rs` (line 233) |
| **Depends on** | GSEC-5 (TPM) |

**What exists:**
```rust
const EVM_HMAC_KEY: &[u8; 32] = b"turnix-evm-hmac-key-2024-v1!1234";
```

**What's needed:**
- TPM driver to store/derive keys
- Boot-time key loading from TPM
- Key rotation support
- Remove hardcoded constant

---

### GCUT-2: Unix Socket Backlog Hardcoded
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P3 — Low |
| **Effort** | S (hours) |
| **Files** | `kernel/src/ipc/unix_socket.rs` (line 189) |

**What exists:**
```rust
backlog: 5,  // hardcoded
```

**What's needed:**
- Use `backlog` parameter from `listen()` call
- Validate backlog range (1..SOMAXCONN)

---

### GCUT-3: fork/clone Placeholder Syscalls
| | |
|---|---|
| **Status** | NOT STARTED |
| **Priority** | P3 — Low |
| **Effort** | S (hours) |
| **Files** | `kernel/src/syscall/handler.rs` (lines 819–827) |

**What exists:**
- `handle_fork()` and `handle_clone()` return `Error(0)` if called directly
- Real implementations are in `handle_fork_with_frame` / `handle_clone_with_frame`

**What's needed:**
- Either route these to the real implementations or add clear panic with diagnostic message
- Ensure these are never dispatched in practice

---

## Implementation Dependency Graph

```
                    ┌─────────────┐
                    │  GFS-1      │
                    │ Block Device │
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
        ┌─────┴─────┐ ┌───┴───┐ ┌─────┴─────┐
        │  GFS-2    │ │ GFS-3 │ │  GNET-2   │
        │ ext4 R/W  │ │ext2 R/W│ │ Net IF Cfg│
        └─────┬─────┘ └───┬───┘ └─────┬─────┘
              │            │            │
        ┌─────┴─────┐     │      ┌─────┴─────┐
        │  GFS-4    │     │      │  GNET-1   │
        │ VFS xattr │     │      │   DHCP    │
        └─────┬─────┘     │      └───────────┘
              │            │
    ┌─────────┼─────────┐  │
    │         │         │  │
┌───┴───┐ ┌──┴──┐ ┌────┴──┴──┐
│GSEC-2 │ │GSEC-3│ │ GSEC-4  │
│EVM    │ │File  │ │ MAC     │
│wired  │ │Caps  │ │ Framework│
└───┬───┘ └─────┘ └──────────┘
    │
┌───┴───┐
│GSEC-5 │
│TPM Key│
└───────┘

    ┌──────────────┐
    │   GGPU-1     │
    │ VirtIO-GPU   │
    └──────┬───────┘
           │
    ┌──────┼──────┐
    │             │
┌───┴───┐   ┌────┴────┐
│GGPU-2 │   │ GGPU-3  │
│ EDID  │   │ Double  │
│       │   │ Buffer  │
└───┬───┘   └─────────┘
    │
┌───┴───┐
│GUSR-4 │
│DynRes │
└───────┘
```

---

## Summary Statistics

| Priority | Count | Total Effort |
|----------|-------|--------------|
| P0 — Critical | 8 | ~30–40 weeks |
| P1 — High | 13 | ~30–45 weeks |
| P2 — Medium | 10 | ~12–20 weeks |
| P3 — Low | 3 | ~1–2 days |
| **Total** | **34** | **~75–110 weeks** |

> **Note:** Many tasks can be parallelized. Critical path is GFS-1 → GFS-2 → GSEC-1 → GSEC-2/3/4.
