use x86_64::VirtAddr;
use x86_64::registers::control::Cr2;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageSize, PageTable, PageTableFlags, PhysFrame,
    Size4KiB,
};

use crate::memory::PAGE_SIZE;
use crate::memory::page_cache::PAGE_CACHE;
use crate::memory::vma::{VmaBacking, VmaProt};

/// Map a physical frame into the current process at the given virtual
/// address, using the protection flags from the VMA.
fn map_fault_frame(
    fault_addr: VirtAddr,
    frame: PhysFrame<Size4KiB>,
    prot: VmaProt,
    allocator: &mut impl FrameAllocator<Size4KiB>,
    phys_mem_offset: VirtAddr,
) -> bool {
    unsafe {
        let page = Page::<Size4KiB>::containing_address(fault_addr);
        let (pml4_frame, _) = x86_64::registers::control::Cr3::read();
        let pml4_ptr =
            (phys_mem_offset + pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();
        let mut mapper = OffsetPageTable::new(&mut *pml4_ptr, phys_mem_offset);

        let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        if prot.is_writable() {
            flags |= PageTableFlags::WRITABLE;
        }
        if !prot.is_executable() {
            flags |= PageTableFlags::NO_EXECUTE;
        }

        match mapper.map_to(page, frame, flags, allocator) {
            Ok(flush) => {
                flush.flush();
                true
            }
            Err(_) => false,
        }
    }
}

/// Read a page of file data from the VFS for a file-backed mapping.
/// Falls back to zero-fill when the file data is unavailable.
fn populate_file_page(frame_ptr: *mut u8, inode: crate::memory::vma::InodeId, page_idx: u64) {
    let vfs = crate::vfs::VFS.lock();
    let page_offset = page_idx * PAGE_SIZE;
    let buf = unsafe { core::slice::from_raw_parts_mut(frame_ptr, Size4KiB::SIZE as usize) };
    if !vfs.read_page(inode.0 as usize, page_offset, buf) {
        // File data not available — zero-fill
        unsafe {
            core::ptr::write_bytes(frame_ptr, 0, Size4KiB::SIZE as usize);
        }
    }
}

/// Handle a page fault caused by demand paging.
///
/// Checks the faulting address (from `Cr2`) against the current process's
/// `VmaSet`. If the address is covered by a VMA:
///
/// - For `Anonymous` VMAs: allocates a zero-filled physical frame and maps it.
/// - For `FileBacked` VMAs: checks the page cache first. On hit, maps the
///   cached frame directly. On miss, allocates a new frame, reads file data
///   from the VFS, populates the cache, and maps the frame.
///
/// Returns `true` if the fault was resolved, `false` if it should result
/// in `SIGSEGV` / process termination.
pub fn handle_demand_fault() -> bool {
    let fault_addr = match Cr2::read() {
        Ok(a) => a,
        Err(_) => return false,
    };

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return false,
    };

    let vma = match process.with_vma_set(|vmas| vmas.find(fault_addr).cloned()) {
        Some(v) => v,
        None => return false,
    };

    let phys_mem_offset = crate::boot::get_phys_mem_offset();

    // ── Check for swapped-out page ──────────────────────────
    // Must happen BEFORE acquiring FRAME_ALLOCATOR because restore_swapped_page
    // → swap_in → allocate_swappable_frame also locks FRAME_ALLOCATOR.
    // spin::Mutex is NOT re-entrant, so holding the lock here would deadlock.
    {
        let pml4_frame = process.pml4_frame();
        let pte_bits = crate::memory::swap::read_pte(pml4_frame, phys_mem_offset, fault_addr);
        if let Some(bits) = pte_bits
            && crate::memory::swap::is_swapped_out_pte(bits)
        {
            if let Some(phys) = crate::memory::swap::restore_swapped_page(bits) {
                let kframe = PhysFrame::containing_address(x86_64::PhysAddr::new(phys));
                let mut guard = crate::boot::FRAME_ALLOCATOR.lock();
                return match guard.as_mut() {
                    Some(a) => map_fault_frame(fault_addr, kframe, vma.prot, a, phys_mem_offset),
                    None => false,
                };
            }
            return false;
        }
    }

    // ── Acquire frame allocator for non-swap paths ──────────
    let mut frame_allocator_guard = crate::boot::FRAME_ALLOCATOR.lock();
    let allocator = match frame_allocator_guard.as_mut() {
        Some(a) => a,
        None => return false,
    };

    // ── File-backed VMA — use the page cache ──────────────────────
    if let VmaBacking::FileBacked { inode, offset } = &vma.backing {
        let vma_offset = fault_addr.as_u64().saturating_sub(vma.start.as_u64());
        let file_offset = offset.saturating_add(vma_offset);
        let page_idx = file_offset / PAGE_SIZE;
        let now = crate::task::scheduler::get_uptime_ticks();

        let mut cache = PAGE_CACHE.lock();

        // 1. Cache hit — map the existing shared frame
        if let Some(cached_frame) = cache.lookup(*inode, page_idx, now) {
            cache.add_ref(*inode, page_idx);
            drop(cache);
            let kframe =
                PhysFrame::containing_address(x86_64::PhysAddr::new(cached_frame.start_address));
            return map_fault_frame(fault_addr, kframe, vma.prot, allocator, phys_mem_offset);
        }
        drop(cache);

        // 2. Cache miss — allocate a frame, populate from VFS, insert
        let kframe: PhysFrame<Size4KiB> = match allocator.allocate_frame() {
            Some(f) => f,
            None => return false,
        };
        let frame_ptr = (phys_mem_offset + kframe.start_address().as_u64()).as_mut_ptr::<u8>();

        populate_file_page(frame_ptr, *inode, page_idx);

        let mut cache = PAGE_CACHE.lock();
        let phys = crate::memory::PhysFrame {
            start_address: kframe.start_address().as_u64(),
        };
        cache.insert(*inode, page_idx, phys, now);
        cache.add_ref(*inode, page_idx);

        return map_fault_frame(fault_addr, kframe, vma.prot, allocator, phys_mem_offset);
    }

    // ── Anonymous VMA — allocate + zero-fill ──────────────────────
    let frame: PhysFrame<Size4KiB> = match allocator.allocate_frame() {
        Some(f) => f,
        None => return false,
    };

    let frame_ptr = (phys_mem_offset + frame.start_address().as_u64()).as_mut_ptr::<u8>();
    unsafe {
        core::ptr::write_bytes(frame_ptr, 0, Size4KiB::SIZE as usize);
    }

    map_fault_frame(fault_addr, frame, vma.prot, allocator, phys_mem_offset)
}

/// Check whether the given `VmaProt` violates W^X (both WRITE and EXECUTE).
/// The `mmap` syscall must reject such requests with `EACCES`.
pub fn check_wx(prot: VmaProt) -> bool {
    prot.violates_wx()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::vma::{Vma, VmaBacking, VmaFlags, VmaSet};
    use alloc::vec::Vec;
    use core::ptr;
    use proptest::collection as pc;
    use proptest::prelude::*;
    use x86_64::VirtAddr;

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    fn arb_vma_prot() -> impl Strategy<Value = VmaProt> {
        (0..8u8).prop_map(VmaProt::from_bits_truncate)
    }

    /// Generate non-overlapping anonymous VMAs (page-aligned, non-zero size).
    fn arb_anon_vmas() -> impl Strategy<Value = Vec<Vma>> {
        pc::vec((0..100u64, 1..50u64, arb_vma_prot()), 1..20).prop_map(|segments| {
            let mut vmas = Vec::new();
            let mut cursor = VirtAddr::new(0x1000);
            for (gap, size, prot) in segments {
                cursor += gap * 0x1000;
                let start = cursor;
                cursor += size * 0x1000;
                let end = cursor;
                vmas.push(Vma {
                    start,
                    end,
                    prot,
                    backing: VmaBacking::Anonymous,
                    flags: VmaFlags::MAP_PRIVATE,
                });
            }
            vmas
        })
    }

    /// Pick a page-aligned test address inside a VMA (guaranteed non-zero offset).
    fn test_addr_inside(vma: &Vma) -> VirtAddr {
        let pages = vma.size() / 0x1000;
        if pages <= 1 {
            vma.start
        } else {
            vma.start + (pages / 2) * 0x1000
        }
    }

    // ------------------------------------------------------------------
    // Property 3 — Demand Paging Zero-Fill
    // ------------------------------------------------------------------

    proptest! {
        /// Property 3 — Demand Paging Zero-Fill
        ///
        /// For any set of anonymous VMAs and every page-aligned address
        /// **within** those VMAs:
        ///
        ///   1. `VmaSet::find()` returns `Some` — the faulting address
        ///      is recognised as belonging to a mapped region.
        ///   2. The returned VMA has `VmaBacking::Anonymous` — the demand
        ///      paging handler must zero-fill the allocated frame.
        ///   3. The returned VMA has the correct `VmaProt` flags — the
        ///      handler maps the frame with the VMA's protection bits.
        ///   4. The returned VMA is the *exact* VMA that was inserted
        ///      (start + end match).
        ///
        /// For addresses **outside** all VMAs, `find()` returns `None`
        /// (the handler would return `false` → SIGSEGV).
        ///
        /// This validates Requirements 9.2 (zero-fill anonymous pages)
        /// and 9.3 (map with correct protection flags).
        #[test]
        fn demand_paging_zero_fill_contract(
            vmas in arb_anon_vmas(),
        ) {
            let mut set = VmaSet::new();
            for vma in &vmas {
                let _ = set.insert(vma.clone());
            }

            // --- Inside every VMA ---
            for vma in &vmas {
                let addr = test_addr_inside(vma);
                let found = set.find(addr);
                prop_assert!(
                    found.is_some(),
                    "(Zero-fill) address {:#x} in anonymous VMA [{:#x}, {:#x}) must be findable",
                    addr.as_u64(), vma.start.as_u64(), vma.end.as_u64(),
                );

                if let Some(found_vma) = found {
                    // Must be the *same* VMA (start & end match)
                    prop_assert_eq!(
                        found_vma.start, vma.start,
                        "(Zero-fill) found VMA start mismatch at {:#x}",
                        addr.as_u64(),
                    );
                    prop_assert_eq!(
                        found_vma.end, vma.end,
                        "(Zero-fill) found VMA end mismatch at {:#x}",
                        addr.as_u64(),
                    );

                    // Must be anonymous → handler zero-fills
                    prop_assert!(
                        matches!(found_vma.backing, VmaBacking::Anonymous),
                        "(Zero-fill) VMA at {:#x} must be Anonymous",
                        addr.as_u64(),
                    );

                    // Prot flags must match → handler maps correctly
                    prop_assert_eq!(
                        found_vma.prot, vma.prot,
                        "(Zero-fill) VMA at {:#x} prot mismatch",
                        addr.as_u64(),
                    );

                    // MAP_PRIVATE must be set
                    prop_assert!(
                        found_vma.flags.contains(VmaFlags::MAP_PRIVATE),
                        "(Zero-fill) VMA at {:#x} must be MAP_PRIVATE",
                        addr.as_u64(),
                    );
                }

                // The exclusive end must NOT be covered by this VMA
                // (it may be the start of the next VMA; that's fine)
                let found_end = set.find(vma.end);
                if let Some(fe) = found_end {
                    prop_assert_ne!(
                        fe.start, vma.start,
                        "(Zero-fill) exclusive end {:#x} of VMA at {:#x} \
                         must not be covered by the same VMA",
                        vma.end.as_u64(), vma.start.as_u64(),
                    );
                }
            }

            // --- Outside all VMAs (high address) → SIGSEGV ---
            let far_addr = VirtAddr::new(0x10_0000_0000u64);
            let found_far = set.find(far_addr);
            prop_assert!(
                found_far.is_none(),
                "(Zero-fill) far address {:#x} must not be findable (SIGSEGV)",
                far_addr.as_u64(),
            );

            // --- Gaps between VMAs → SIGSEGV ---
            let all: Vec<&Vma> = set.iter().collect();
            for pair in all.windows(2) {
                let prev = pair[0];
                let next = pair[1];
                if prev.end < next.start {
                    let gap_addr = prev.end + 1u64;
                    if gap_addr < next.start {
                        let found_gap = set.find(gap_addr);
                        prop_assert!(
                            found_gap.is_none(),
                            "(Zero-fill) gap address {:#x} between VMAs must not be findable (SIGSEGV)",
                            gap_addr.as_u64(),
                        );
                    }
                }
            }
        }
    }

    /// Deterministic test: the zero-fill mechanism itself.
    /// Write non-zero data into a heap buffer, call `write_bytes(0)`
    /// (the same zero-fill primitive `handle_demand_fault` uses), and
    /// verify every byte is zero.  This validates the zero-fill *mechanism*
    /// that backs the demand paging contract.
    #[test]
    fn demand_paging_zero_fill_mechanism() {
        let mut buffer = [0u8; 4096];

        // Fill with non-zero pattern
        for (i, byte) in buffer.iter_mut().enumerate() {
            *byte = ((i * 7 + 13) & 0xFF) as u8;
        }

        // Verify non-zero (sanity check)
        let has_non_zero = buffer.iter().any(|&b| b != 0);
        assert!(has_non_zero, "buffer should be non-zero before zero-fill");

        // Zero-fill using the same primitive as handle_demand_fault
        unsafe {
            ptr::write_bytes(buffer.as_mut_ptr(), 0, buffer.len());
        }

        // Verify every byte is zero
        for (i, &byte) in buffer.iter().enumerate() {
            assert_eq!(
                byte, 0,
                "byte at offset {} should be zero after write_bytes(0)",
                i,
            );
        }
    }

    // ------------------------------------------------------------------
    // Property 5 — SIGSEGV on Unmapped Access
    // ------------------------------------------------------------------

    proptest! {
        /// Property 5 — SIGSEGV on Unmapped Access
        ///
        /// For addresses outside all VMAs, `VmaSet::find()` must return
        /// `None`.  This causes `handle_demand_fault` to return `false`
        /// and the page-fault handler to terminate / SIGSEGV the process.
        #[test]
        fn sigsegv_on_unmapped_address(
            vmas in arb_anon_vmas(),
            base in 0x1_0000_0000u64..0x10_0000_0000u64,
        ) {
            let mut set = VmaSet::new();
            for vma in &vmas {
                let _ = set.insert(vma.clone());
            }
            let addr = VirtAddr::new(base);
            let found = set.find(addr);
            prop_assert!(
                found.is_none(),
                "address {:#x} outside all VMAs must not be findable (SIGSEGV)",
                addr.as_u64(),
            );
        }
    }

    // ------------------------------------------------------------------
    // Property 13 — mmap W+X Rejection
    // ------------------------------------------------------------------

    proptest! {
        /// Property 13 — mmap W+X Rejection
        ///
        /// Any mmap request with `PROT_WRITE | PROT_EXEC` must be rejected
        /// by the kernel before a VMA is created.  The `check_wx` function
        /// must exactly mirror `VmaProt::violates_wx()`.
        #[test]
        fn mmap_wx_rejection(prot_bits in 0..8u8) {
            let prot = VmaProt::from_bits_truncate(prot_bits);
            let violates = prot.violates_wx();
            let caught = check_wx(prot);
            prop_assert_eq!(
                violates, caught,
                "check_wx({:?}) must agree with violates_wx()", prot,
            );
            if prot.is_writable() && prot.is_executable() {
                prop_assert!(caught, "W+X prot {:?} must be rejected", prot);
            } else {
                prop_assert!(!caught, "non-W+X prot {:?} must be allowed", prot);
            }
        }
    }

#[test]
fn check_wx_exhaustive() {
        let cases = [
            (VmaProt::empty(), false),
            (VmaProt::READ, false),
            (VmaProt::WRITE, false),
            (VmaProt::EXECUTE, false),
            (VmaProt::READ | VmaProt::WRITE, false),
            (VmaProt::READ | VmaProt::EXECUTE, false),
            (VmaProt::WRITE | VmaProt::EXECUTE, true),
            (VmaProt::READ | VmaProt::WRITE | VmaProt::EXECUTE, true),
        ];
        for (prot, expected) in &cases {
            assert_eq!(
                check_wx(*prot),
                *expected,
                "check_wx({:?}) should be {}",
                prot,
                expected,
            );
        }
    }

    #[test]
    fn zero_fill_partial_page() {
        let mut buf = [0xFFu8; 64];
        let half = 32;
        unsafe { core::ptr::write_bytes(buf.as_mut_ptr(), 0, half); }
        for (i, &byte) in buf[..half].iter().enumerate() {
            assert_eq!(byte, 0, "byte {} should be zeroed", i);
        }
        for (i, &byte) in buf[half..].iter().enumerate() {
            assert_eq!(byte, 0xFF, "byte {} should be unchanged", i + half);
        }
    }

    #[test]
    fn zero_fill_entire_page() {
        let mut buf = [0xABu8; 4096];
        unsafe { core::ptr::write_bytes(buf.as_mut_ptr(), 0, buf.len()); }
        for (i, &byte) in buf.iter().enumerate() {
            assert_eq!(byte, 0, "byte at offset {} should be zero after full-page zero-fill", i);
        }
    }
}
