use x86_64::VirtAddr;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr3};
use x86_64::structures::paging::{OffsetPageTable, PageTable, PageTableFlags, PhysFrame, Size4KiB};

/// Initialize a new OffsetPageTable using the mapping provided by the loader.
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let (pml4_frame, _) = Cr3::read();

    let pml4_ptr =
        (physical_memory_offset + pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();

    unsafe { OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset) }
}

/// Creates a new PML4 for a process by cloning the kernel's higher-half mappings.
pub fn create_process_pml4(
    frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<Size4KiB>,
    physical_memory_offset: VirtAddr,
) -> PhysFrame<Size4KiB> {
    let new_frame = frame_allocator
        .allocate_frame()
        .expect("failed to allocate frame for process PML4");

    let (kernel_pml4_frame, _) = Cr3::read();
    let kernel_pml4_ptr =
        (physical_memory_offset + kernel_pml4_frame.start_address().as_u64()).as_ptr::<PageTable>();
    let new_pml4_ptr =
        (physical_memory_offset + new_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();

    unsafe {
        // Initialize new PML4 with zeros
        core::ptr::write_bytes(new_pml4_ptr, 0, 1);

        let kernel_pml4 = &*kernel_pml4_ptr;
        let new_pml4 = &mut *new_pml4_ptr;

        // Clone higher-half mappings (indices 256 to 511)
        // These stay SUPERVISOR-only (no USER bit).
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
                    frame_allocator,
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
    let src_pml4_ptr = (physical_memory_offset + src_pml4_frame.start_address().as_u64())
        .as_ptr::<PageTable>();
    let dst_pml4_ptr = (physical_memory_offset + dst_pml4_frame.start_address().as_u64())
        .as_mut_ptr::<PageTable>();

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
                    frame_allocator,
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
/// Ensure a virtual address range is accessible to user mode (Ring 3).
/// This sets the USER_ACCESSIBLE bit on all page table levels.
pub unsafe fn set_user_accessible(virt_addr: VirtAddr, size: u64, physical_mem_offset: VirtAddr) {
    // Disable write protection to allow modifying RO page tables inherited from UEFI
    unsafe {
        Cr0::update(|f| f.remove(Cr0Flags::WRITE_PROTECT));
    }

    let (pml4_frame, _) = Cr3::read();
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
        let p3 =
            unsafe { &mut *((physical_mem_offset + p3_phys.as_u64()).as_mut_ptr::<PageTable>()) };
        let f3 = p3[p3_idx].flags();
        p3[p3_idx].set_flags(f3 | PageTableFlags::USER_ACCESSIBLE);

        let p2_phys = p3[p3_idx].frame().unwrap().start_address();
        let p2 =
            unsafe { &mut *((physical_mem_offset + p2_phys.as_u64()).as_mut_ptr::<PageTable>()) };
        let f2 = p2[p2_idx].flags();
        p2[p2_idx].set_flags(f2 | PageTableFlags::USER_ACCESSIBLE);

        let p1_phys = p2[p2_idx].frame().unwrap().start_address();
        let p1 =
            unsafe { &mut *((physical_mem_offset + p1_phys.as_u64()).as_mut_ptr::<PageTable>()) };
        let f1 = p1[p1_idx].flags();
        p1[p1_idx].set_flags(f1 | PageTableFlags::USER_ACCESSIBLE);
    }

    // Re-enable write protection
    unsafe {
        Cr0::update(|f| f.insert(Cr0Flags::WRITE_PROTECT));
    }
}
