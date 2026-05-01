# ADR 0001: Kernel Architecture and Memory Model

## Status
Accepted

## Context
turnix needs a robust foundation for a modern desktop operating system. We need to decide on the fundamental kernel model and memory layout to ensure scalability, security, and developer productivity.

## Decision
1. **Higher-Half Kernel**: The kernel is mapped to the higher half of the 64-bit virtual address space (specifically the "negative" half, `0xFFFF_8000_0000_0000` to `0xFFFF_FFFF_FFFF_FFFF`).
   - **Rationale**: This leaves the entire lower half (`0x0` to `0x0000_7FFF_FFFF_FFFF`) available for user-mode processes. It ensures that kernel addresses are identical across all process contexts, eliminating the need for expensive TLB flushes or Page Table switches during system calls.
2. **Physical Memory Direct Map (HHDM)**: All physical RAM is mapped linearly starting at `0xFFFF_8000_0000_0000`.
   - **Rationale**: This allows the kernel to access any physical address by simply adding a constant offset. It simplifies the implementation of page table management and DMA-like operations. We use **2MB Huge Pages** for this region to reduce TLB pressure.
3. **Monolithic Design (Phase 1)**: Core services like Memory Management, Virtual File System (VFS), and the Scheduler run in Ring 0 within the kernel executable.
   - **Rationale**: This maximizes performance and minimizes implementation complexity during the initial boot and stability phases. We maintain a clear internal modularity to allow a future transition to a hybrid or microkernel model if required.
4. **Rust-First Implementation**: The kernel, UEFI loader, and standard libraries are written in Rust.
   - **Rationale**: OS development demands extreme control over memory without the overhead of a garbage collector. Rust provides this while enforcing memory safety and thread safety at compile-time, preventing entire classes of common OS bugs (e.g., use-after-free, data races on GDT access).

## Virtual Address Layout Details

| Virtual Range | Description | Flags |
|---------------|-------------|-------|
| `0x0000_0000_0000_0000` - `0x0000_7FFF_FFFF_FFFF` | **User Space** | `User`, `RW` (varies) |
| `0xFFFF_8000_0000_0000` - `0xFFFF_8FFF_FFFF_FFFF` | **Physical Direct Map** | `Global`, `NX`, `RW` |
| `0xFFFF_9000_0000_0000` - `0xFFFF_9FFF_FFFF_FFFF` | **Ramdisk / Boot Data** | `Global`, `NX`, `RO` |
| `0xFFFF_FE00_0000_0000` - `0xFFFF_FEFF_FFFF_FFFF` | **Kernel Stacks** | `Global`, `NX`, `RW` |
| `0xFFFF_FFFF_8000_0000` - `0xFFFF_FFFF_FFFF_FFFF` | **Kernel Code/Data** | `Global`, `RX`/`RW` |

## Consequences
- **Isolation**: Kernel regions are protected from Ring 3 via the `Supervisor` bit in page table entries. Any unauthorized access triggers a Page Fault.
- **Performance**: System calls are highly efficient as they avoid CR3 switches.
- **Complexity**: The bootloader must correctly set up the initial higher-half mapping and jump to the high-canonical entry point.
