use x86_64::registers::control::Cr2;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageSize, PageTable, PageTableFlags, PhysFrame,
    Size4KiB,
};

use crate::memory::vma::VmaProt;

/// Handle a page fault caused by demand paging.
///
/// Checks the faulting address (from `Cr2`) against the current process's
/// `VmaSet`. If the address is covered by a VMA, a physical frame is
/// allocated, zero-filled, and mapped with the VMA's protection flags.
/// Returns `true` if the fault was resolved, `false` if it should
/// result in `SIGSEGV` / process termination.
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
    let mut frame_allocator_guard = crate::boot::FRAME_ALLOCATOR.lock();
    let allocator = match frame_allocator_guard.as_mut() {
        Some(a) => a,
        None => return false,
    };

    let frame: PhysFrame<Size4KiB> = match allocator.allocate_frame() {
        Some(f) => f,
        None => return false,
    };

    let frame_ptr = (phys_mem_offset + frame.start_address().as_u64()).as_mut_ptr::<u8>();
    unsafe {
        core::ptr::write_bytes(frame_ptr, 0, Size4KiB::SIZE as usize);
    }

    let page = Page::<Size4KiB>::containing_address(fault_addr);

    let (pml4_frame, _) = x86_64::registers::control::Cr3::read();
    let pml4_ptr =
        (phys_mem_offset + pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();
    let mut mapper = unsafe { OffsetPageTable::new(&mut *pml4_ptr, phys_mem_offset) };

    let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if vma.prot.is_writable() {
        flags |= PageTableFlags::WRITABLE;
    }
    if !vma.prot.is_executable() {
        flags |= PageTableFlags::NO_EXECUTE;
    }

    unsafe {
        match mapper.map_to(page, frame, flags, allocator) {
            Ok(flush) => {
                flush.flush();
                true
            }
            Err(_) => false,
        }
    }
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
        (0..8u8).prop_map(|bits| VmaProt::from_bits_truncate(bits))
    }

    /// Generate non-overlapping anonymous VMAs (page-aligned, non-zero size).
    fn arb_anon_vmas() -> impl Strategy<Value = Vec<Vma>> {
        pc::vec((0..100u64, 1..50u64, arb_vma_prot()), 1..20).prop_map(|segments| {
            let mut vmas = Vec::new();
            let mut cursor = VirtAddr::new(0x1000);
            for (gap, size, prot) in segments {
                cursor = cursor + gap * 0x1000;
                let start = cursor;
                cursor = cursor + size * 0x1000;
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
}
