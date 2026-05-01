# turnix Memory Model: The Higher-Half Design

turnix utilizes a "Higher-Half" kernel design, which is a standard pattern in SOTA operating systems like Linux and Windows. This document explains the virtual memory layout and why these decisions were made.

## Virtual Address Space Layout (x86_64)

The 64-bit virtual address space is split into two halves:

1.  **Lower-Half (`0x0000_0000_0000_0000` - `0x0000_7FFF_FFFF_FFFF`)**: Reserved for **User Space** (Ring 3). Each process has its own unique lower-half mapping.
2.  **Higher-Half (`0xFFFF_8000_0000_0000` - `0xFFFF_FFFF_FFFF_FFFF`)**: Reserved for **Kernel Space** (Ring 0). These mappings are global and shared across all process address spaces.

### Key Higher-Half Regions

| Virtual Address Range | Purpose |
|-----------------------|---------|
| `0xFFFF_8000_0000_0000` | **Physical Memory Direct Map**. The first 4GB (expandable) of RAM is mapped here. |
| `0xFFFF_9000_0000_0000` | **Initramfs / Ramdisk**. The boot-time ramdisk is mapped here. |
| `0xFFFF_FE00_0000_0000` | **Kernel Stacks**. Dynamically allocated stacks for kernel threads. |
| `0xFFFF_FFFF_8000_0000` | **Kernel Executable**. The kernel ELF segments are mapped here. |

## Why Higher-Half?

1.  **Simplicity in Syscalls**: When a user process makes a system call, the CPU switches to Ring 0 but *stays in the same address space*. Because the kernel is already mapped in the higher-half, we don't need to perform a costly CR3 (Page Table) switch for every syscall.
2.  **Universal Kernel Access**: The kernel can access its own code and data regardless of which user process is currently scheduled.
3.  **Physical Memory Direct Map**: Mapping all physical RAM to a specific higher-half offset allows the kernel to manipulate page tables and hardware structures using simple pointer arithmetic (e.g., `PhysAddr + Offset = VirtAddr`).

## Page Table Implementation

turnix uses 4-level paging (`PML4` -> `PDPT` -> `PD` -> `PT`). 
- For efficiency, the **Physical Memory Direct Map** uses **2MB Huge Pages**, reducing TLB pressure and improving performance (SOTA optimization).
- User space and kernel executable regions use standard **4KB pages** for fine-grained protection.

## Safety and Isolation

Even though the kernel is mapped into the user address space, it is protected by the `Supervisor` bit in the page tables. 
## Related Decisions
For the high-level rationale and architectural implications, see [ADR 0001: Kernel Architecture and Memory Model](decisions/0001-kernel-architecture.md).
