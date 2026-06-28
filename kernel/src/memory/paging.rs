use x86_64::VirtAddr;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr3};
use x86_64::structures::paging::{
    FrameAllocator, OffsetPageTable, PageTable, PageTableFlags, PhysFrame, Size4KiB,
};

// Force an extra symbol to shift code layout and avoid a latent LLVM alignment ICE.
#[used]
static PAGING_PAD: u64 = 0;

/// A frame allocator wrapper that zeroes every allocated frame before returning
/// it. This is required for page table frames: the x86_64 mapper reads entries
/// before writing them, so stale data in recycled frames causes spurious faults
/// (and WHPX VP-exit-4 crashes).
struct ZeroingFrameAllocator<'a, A: x86_64::structures::paging::FrameAllocator<Size4KiB>> {
    inner: &'a mut A,
    physical_memory_offset: VirtAddr,
}

unsafe impl<'a, A: x86_64::structures::paging::FrameAllocator<Size4KiB>>
    x86_64::structures::paging::FrameAllocator<Size4KiB> for ZeroingFrameAllocator<'a, A>
{
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let frame = self.inner.allocate_frame()?;
        // Zero the frame through the physical-memory window before use.
        let ptr = (self.physical_memory_offset + frame.start_address().as_u64()).as_mut_ptr::<u8>();
        // SAFETY: ptr is derived from a valid physical frame address plus the
        // physical-memory offset, which maps to a mapped page. The frame was
        // just allocated and is therefore valid for 4096 bytes of writes.
        unsafe { core::ptr::write_bytes(ptr, 0, 4096) };
        Some(frame)
    }
}

/// Initialize a new OffsetPageTable using the mapping provided by the loader.
///
/// # Safety
///
/// `physical_memory_offset` must be a valid offset for the active page tables.
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let (pml4_frame, _) = Cr3::read();

    let pml4_ptr =
        (physical_memory_offset + pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();

    // SAFETY: pml4_ptr is derived from the active PML4 frame (read via Cr3)
    // plus the physical-memory offset, which is valid per the caller's safety
    // contract. The reference is valid for the page table.
    unsafe { OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset) }
}

/// Creates a new PML4 for a process by cloning the kernel's higher-half mappings.
pub fn create_process_pml4(
    frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<Size4KiB>,
    physical_memory_offset: VirtAddr,
) -> PhysFrame<Size4KiB> {
    let mut zeroing = ZeroingFrameAllocator {
        inner: frame_allocator,
        physical_memory_offset,
    };
    let new_frame = zeroing
        .allocate_frame()
        .expect("failed to allocate frame for process PML4");

    let (kernel_pml4_frame, _) = Cr3::read();
    let kernel_pml4_ptr =
        (physical_memory_offset + kernel_pml4_frame.start_address().as_u64()).as_ptr::<PageTable>();
    let new_pml4_ptr =
        (physical_memory_offset + new_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();

    unsafe {
        let kernel_pml4 = &*kernel_pml4_ptr;
        let new_pml4 = &mut *new_pml4_ptr;

        for i in 256..512 {
            new_pml4[i] = kernel_pml4[i].clone();
        }
    }

    new_frame
}

/// Clones the lower-half (user) mappings from one address space to another.
pub fn clone_user_mappings(
    src_pml4_frame: PhysFrame<Size4KiB>,
    dst_pml4_frame: PhysFrame<Size4KiB>,
    frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<Size4KiB>,
    physical_memory_offset: VirtAddr,
    cow: bool,
) {
    let src_pml4_ptr =
        (physical_memory_offset + src_pml4_frame.start_address().as_u64()).as_ptr::<PageTable>();
    let dst_pml4_ptr = (physical_memory_offset + dst_pml4_frame.start_address().as_u64())
        .as_mut_ptr::<PageTable>();

    let mut zeroing = ZeroingFrameAllocator {
        inner: frame_allocator,
        physical_memory_offset,
    };
    // SAFETY: Both src and dst PML4 pointers are derived from valid frame
    // addresses plus the physical-memory offset. The source frame is an
    // existing page table and the destination was freshly allocated and zeroed.
    // Only lower-half indices (0..256) are accessed.
    unsafe {
        let src_pml4 = &*src_pml4_ptr;
        let dst_pml4 = &mut *dst_pml4_ptr;

        // Clone lower-half mappings (indices 0 to 255)
        for i in 0..256 {
            if !src_pml4[i].is_unused() {
                clone_table_level(
                    &src_pml4[i],
                    &mut dst_pml4[i],
                    3, // Start at P4 entry (Level 3 in recursion)
                    &mut zeroing,
                    physical_memory_offset,
                    cow,
                );
            }
        }
    }
}

use x86_64::structures::paging::page_table::PageTableEntry;

/// Recursively clones a page table level.
fn clone_table_level(
    src_entry: &PageTableEntry,
    dst_entry: &mut PageTableEntry,
    level: u8,
    frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<Size4KiB>,
    physical_memory_offset: VirtAddr,
    cow: bool,
) {
    if level == 0 {
        // Leaf entry (Level 0 is P1/Page Table)
        if cow {
            // For COW: mark as read-only and set COW flag (bit 9 is available)
            let mut flags = src_entry.flags();
            flags.remove(PageTableFlags::WRITABLE);
            flags.insert(PageTableFlags::BIT_9); // Mark as COW
            *dst_entry = src_entry.clone();
            dst_entry.set_flags(flags);
        } else {
            *dst_entry = src_entry.clone();
        }
        return;
    }

    // Allocate a new frame for the next level table
    let new_frame = frame_allocator
        .allocate_frame()
        .expect("failed to allocate frame for page table clone");

    // Initialize the entry to point to the new frame
    dst_entry.set_frame(new_frame, src_entry.flags());

    let src_next_table_ptr = (physical_memory_offset
        + src_entry.frame().unwrap().start_address().as_u64())
    .as_ptr::<PageTable>();
    let dst_next_table_ptr =
        (physical_memory_offset + new_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();

    // SAFETY: src_next_table_ptr is derived from a valid source page table
    // entry's frame (verified non-unused via unwrap) plus the physical-memory
    // offset. dst_next_table_ptr is from a freshly allocated frame plus the
    // same offset. Both are valid for 512-entry page table access.
    unsafe {
        let src_next_table = &*src_next_table_ptr;
        let dst_next_table = &mut *dst_next_table_ptr;

        for i in 0..512 {
            if !src_next_table[i].is_unused() {
                clone_table_level(
                    &src_next_table[i],
                    &mut dst_next_table[i],
                    level - 1,
                    frame_allocator,
                    physical_memory_offset,
                    cow,
                );
            } else {
                dst_next_table[i].set_unused();
            }
        }
    }
}

/// Clone user address space with Copy-on-Write support
pub fn clone_user_mappings_cow(
    src_pml4_frame: PhysFrame<Size4KiB>,
    dst_pml4_frame: PhysFrame<Size4KiB>,
    frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<Size4KiB>,
    physical_memory_offset: VirtAddr,
) {
    let src_pml4_ptr =
        (physical_memory_offset + src_pml4_frame.start_address().as_u64()).as_ptr::<PageTable>();
    let dst_pml4_ptr = (physical_memory_offset + dst_pml4_frame.start_address().as_u64())
        .as_mut_ptr::<PageTable>();

    let mut zeroing = ZeroingFrameAllocator {
        inner: frame_allocator,
        physical_memory_offset,
    };
    // SAFETY: Both src and dst PML4 pointers are derived from valid frame
    // addresses plus the physical-memory offset. The source frame is an
    // existing page table and the destination was freshly allocated and zeroed.
    // All 512 entries are accessed within bounds.
    unsafe {
        let src_pml4 = &*src_pml4_ptr;
        let dst_pml4 = &mut *dst_pml4_ptr;

        // Clone lower-half mappings (indices 0 to 255) with COW
        for i in 0..256 {
            if !src_pml4[i].is_unused() {
                clone_table_level(
                    &src_pml4[i],
                    &mut dst_pml4[i],
                    3, // Start at P4 entry (Level 3 in recursion)
                    &mut zeroing,
                    physical_memory_offset,
                    true, // Enable COW
                );
            }
        }

        // Clone higher-half kernel mappings (shared, not COW)
        for i in 256..512 {
            dst_pml4[i] = src_pml4[i].clone();
        }
    }
}

/// Destroy a user address space by recursively freeing user mappings and the
/// page-table frames that back them.
pub fn destroy_user_mappings(
    pml4_frame: PhysFrame<Size4KiB>,
    frame_allocator: &mut crate::memory::FrameAllocator<'_>,
    physical_memory_offset: VirtAddr,
) {
    let pml4_ptr =
        (physical_memory_offset + pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();

    // SAFETY: pml4_ptr is derived from a valid frame address (the process
    // PML4) plus the physical-memory offset. We only iterate over user-space
    // entries (0..256) which are within bounds of the 512-entry page table.
    unsafe {
        let pml4 = &mut *pml4_ptr;
        for index in 0..256 {
            if !pml4[index].is_unused() {
                destroy_table_level(&mut pml4[index], 3, frame_allocator, physical_memory_offset);
            }
        }
    }

    frame_allocator.deallocate_physical_frame(crate::memory::PhysFrame {
        start_address: pml4_frame.start_address().as_u64(),
    });
}

fn destroy_table_level(
    entry: &mut PageTableEntry,
    level: u8,
    frame_allocator: &mut crate::memory::FrameAllocator<'_>,
    physical_memory_offset: VirtAddr,
) {
    if entry.is_unused() {
        return;
    }

    if level == 0 {
        if let Ok(frame) = entry.frame() {
            frame_allocator.deallocate_physical_frame(crate::memory::PhysFrame {
                start_address: frame.start_address().as_u64(),
            });
        }
        entry.set_unused();
        return;
    }

    if entry.flags().contains(PageTableFlags::HUGE_PAGE) {
        if let Ok(frame) = entry.frame() {
            frame_allocator.deallocate_physical_frame(crate::memory::PhysFrame {
                start_address: frame.start_address().as_u64(),
            });
        }
        entry.set_unused();
        return;
    }

    let next_frame = match entry.frame() {
        Ok(frame) => frame,
        Err(_) => {
            entry.set_unused();
            return;
        }
    };
    let next_ptr =
        (physical_memory_offset + next_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();

    // SAFETY: next_ptr is derived from a valid page table entry's frame
    // (verified non-unused and non-huge) plus the physical-memory offset.
    // The pointer is valid for 512-entry page table access and we only read
    // and recurse into non-unused entries.
    unsafe {
        let next_table = &mut *next_ptr;
        for index in 0..512 {
            if !next_table[index].is_unused() {
                destroy_table_level(
                    &mut next_table[index],
                    level - 1,
                    frame_allocator,
                    physical_memory_offset,
                );
            }
        }
    }

    frame_allocator.deallocate_physical_frame(crate::memory::PhysFrame {
        start_address: next_frame.start_address().as_u64(),
    });
    entry.set_unused();
}
/// Ensure a virtual address range is accessible to user mode (Ring 3).
/// This sets the USER_ACCESSIBLE bit on all page table levels.
///
/// # Safety
///
/// Page tables must be mapped at `physical_mem_offset` and `virt_addr`/`size` must be valid.
pub unsafe fn set_user_accessible(virt_addr: VirtAddr, size: u64, physical_mem_offset: VirtAddr) {
    // SAFETY: We temporarily disable write protection to modify page table
    // entries that may have been set read-only by UEFI firmware. This runs
    // with interrupts implicitly disabled (called during process setup).
    // Disable write protection to allow modifying RO page tables inherited from UEFI
    unsafe {
        Cr0::update(|f| f.remove(Cr0Flags::WRITE_PROTECT));
    }

    let (pml4_frame, _) = Cr3::read();
    // SAFETY: The pointer is derived from the active PML4 frame (Cr3) plus
    // the physical-memory offset, which is valid per the caller's safety
    // contract. The reference is valid for 512-entry page table access.
    let pml4 = unsafe {
        &mut *((physical_mem_offset + pml4_frame.start_address().as_u64())
            .as_mut_ptr::<PageTable>())
    };

    let start = virt_addr.as_u64();
    let end = start + size;

    for addr in (start..end).step_by(4096) {
        let v = VirtAddr::new(addr);
        let p4_idx = v.p4_index();
        let p3_idx = v.p3_index();
        let p2_idx = v.p2_index();
        let p1_idx = v.p1_index();

        let f4 = pml4[p4_idx].flags();
        pml4[p4_idx].set_flags(f4 | PageTableFlags::USER_ACCESSIBLE);

        let p3_phys = pml4[p4_idx].frame().unwrap().start_address();
        // SAFETY: p3_phys comes from a valid P4 entry's frame (verified via
        // unwrap). The pointer is valid for 512-entry page table access.
        let p3 =
            unsafe { &mut *((physical_mem_offset + p3_phys.as_u64()).as_mut_ptr::<PageTable>()) };
        let f3 = p3[p3_idx].flags();
        p3[p3_idx].set_flags(f3 | PageTableFlags::USER_ACCESSIBLE);

        let p2_phys = p3[p3_idx].frame().unwrap().start_address();
        // SAFETY: p2_phys comes from a valid P3 entry's frame (verified via
        // unwrap). The pointer is valid for 512-entry page table access.
        let p2 =
            unsafe { &mut *((physical_mem_offset + p2_phys.as_u64()).as_mut_ptr::<PageTable>()) };
        let f2 = p2[p2_idx].flags();
        p2[p2_idx].set_flags(f2 | PageTableFlags::USER_ACCESSIBLE);

        let p1_phys = p2[p2_idx].frame().unwrap().start_address();
        // SAFETY: p1_phys comes from a valid P2 entry's frame (verified via
        // unwrap). The pointer is valid for 512-entry page table access.
        let p1 =
            unsafe { &mut *((physical_mem_offset + p1_phys.as_u64()).as_mut_ptr::<PageTable>()) };
        let f1 = p1[p1_idx].flags();
        p1[p1_idx].set_flags(f1 | PageTableFlags::USER_ACCESSIBLE);
    }

    // SAFETY: Re-enabling write protection after all page table modifications
    // are complete. This restores the normal kernel memory protection.
    // Re-enable write protection
    unsafe {
        Cr0::update(|f| f.insert(Cr0Flags::WRITE_PROTECT));
    }
}

use core::sync::atomic::{AtomicU64, Ordering};

/// Virtual address bump allocator for MMIO regions.
/// Starts at 0xFFFF_C000_0000_0000 and grows upward.
static MMIO_VIRT_NEXT: AtomicU64 = AtomicU64::new(0xFFFF_C000_0000_0000);

/// Map a physical MMIO region into kernel virtual address space using 4 KiB pages.
///
/// Returns the virtual address corresponding to `phys_addr`. The mapping uses
/// `NO_CACHE | WRITABLE | PRESENT` flags appropriate for device MMIO.
///
/// # Safety
///
/// `phys_addr` must refer to a valid MMIO region and `size` must not exceed
/// the region's actual length. The caller must ensure the returned virtual
/// address is used only for volatile MMIO accesses.
pub unsafe fn map_mmio_region(phys_addr: u64, size: u64, phys_mem_offset: VirtAddr) -> VirtAddr {
    let phys_start = phys_addr & !0xFFF;
    let phys_end = (phys_addr + size + 0xFFF) & !0xFFF;
    let num_pages = (phys_end - phys_start) / 4096;

    // Allocate virtual address range
    let virt_start = MMIO_VIRT_NEXT.fetch_add(num_pages * 4096, Ordering::Relaxed);
    let offset_in_page = phys_addr & 0xFFF;

    // Disable write protection to modify page tables
    unsafe {
        Cr0::update(|f| f.remove(Cr0Flags::WRITE_PROTECT));
    }

    let (pml4_frame, _) = Cr3::read();
    let pml4 = unsafe {
        &mut *((phys_mem_offset + pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>())
    };

    for page_idx in 0..num_pages {
        let vaddr = VirtAddr::new(virt_start + page_idx * 4096);
        let paddr = phys_start + page_idx * 4096;

        let p4_idx = vaddr.p4_index();
        let p3_idx = vaddr.p3_index();
        let p2_idx = vaddr.p2_index();
        let p1_idx = vaddr.p1_index();

        // Ensure PDPT exists (level 4 → level 3)
        if pml4[p4_idx].is_unused() {
            let frame = crate::boot::FRAME_ALLOCATOR
                .lock()
                .as_mut()
                .expect("FRAME_ALLOCATOR not initialized")
                .allocate_frame()
                .expect("failed to allocate PDPT for MMIO mapping");
            pml4[p4_idx].set_frame(
                frame,
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
            );
            let pdpt_ptr = (phys_mem_offset + frame.start_address().as_u64()).as_mut_ptr::<PageTable>();
            unsafe { core::ptr::write_bytes(pdpt_ptr, 0, 1) };
        }
        let pdpt = unsafe {
            &mut *((phys_mem_offset + pml4[p4_idx].frame().unwrap().start_address().as_u64())
                .as_mut_ptr::<PageTable>())
        };

        // Ensure PD exists (level 3 → level 2)
        if pdpt[p3_idx].is_unused() {
            let frame = crate::boot::FRAME_ALLOCATOR
                .lock()
                .as_mut()
                .expect("FRAME_ALLOCATOR not initialized")
                .allocate_frame()
                .expect("failed to allocate PD for MMIO mapping");
            pdpt[p3_idx].set_frame(
                frame,
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
            );
            let pd_ptr = (phys_mem_offset + frame.start_address().as_u64()).as_mut_ptr::<PageTable>();
            unsafe { core::ptr::write_bytes(pd_ptr, 0, 1) };
        }
        let pd = unsafe {
            &mut *((phys_mem_offset + pdpt[p3_idx].frame().unwrap().start_address().as_u64())
                .as_mut_ptr::<PageTable>())
        };

        // Ensure PT exists (level 2 → level 1)
        if pd[p2_idx].is_unused() {
            let frame = crate::boot::FRAME_ALLOCATOR
                .lock()
                .as_mut()
                .expect("FRAME_ALLOCATOR not initialized")
                .allocate_frame()
                .expect("failed to allocate PT for MMIO mapping");
            pd[p2_idx].set_frame(
                frame,
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
            );
            let pt_ptr = (phys_mem_offset + frame.start_address().as_u64()).as_mut_ptr::<PageTable>();
            unsafe { core::ptr::write_bytes(pt_ptr, 0, 1) };
        }
        let pt = unsafe {
            &mut *((phys_mem_offset + pd[p2_idx].frame().unwrap().start_address().as_u64())
                .as_mut_ptr::<PageTable>())
        };

        // Map the page
        let frame = PhysFrame::containing_address(x86_64::PhysAddr::new(paddr));
        let flags = PageTableFlags::PRESENT
            | PageTableFlags::WRITABLE
            | PageTableFlags::NO_CACHE
            | PageTableFlags::WRITE_THROUGH;
        pt[p1_idx].set_frame(frame, flags);
    }

    // Re-enable write protection
    unsafe {
        Cr0::update(|f| f.insert(Cr0Flags::WRITE_PROTECT));
    }

    // Flush TLB for the mapped range
    for page_idx in 0..num_pages {
        let vaddr = VirtAddr::new(virt_start + page_idx * 4096);
        x86_64::instructions::tlb::flush(vaddr);
    }

    VirtAddr::new(virt_start + offset_in_page)
}
