use x86_64::VirtAddr;
use x86_64::instructions::tlb;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{
    OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB,
};

// ---------------------------------------------------------------------------
// W^X enforcement helpers
// ---------------------------------------------------------------------------

/// Enforce W^X on the given page-table flags.
///
/// If the flags contain both `WRITABLE` and are executable (i.e. `NO_EXECUTE`
/// is **not** set), `WRITABLE` is stripped so the resulting entry is never
/// simultaneously writable and executable.
///
/// Returns `true` if a violation was detected and corrected.
pub fn enforce_wx_on_flags(flags: &mut PageTableFlags) -> bool {
    let is_wx =
        flags.contains(PageTableFlags::WRITABLE) && !flags.contains(PageTableFlags::NO_EXECUTE);
    if is_wx {
        flags.remove(PageTableFlags::WRITABLE);
    }
    is_wx
}

/// Recursively walk a page table counting violations of W^X.
///
/// `level` is the depth (4 = PML4, 1 = P1 / leaf).  Returns the number
/// of leaf entries that are both `WRITABLE` and executable (no `NO_EXECUTE`).
fn count_wx_violations(table: &PageTable, level: u8, phys_mem_offset: VirtAddr) -> usize {
    let mut violations = 0;

    for i in 0..512 {
        let entry = &table[i];
        if entry.is_unused() {
            continue;
        }

        let flags = entry.flags();

        // Leaf detection:
        //   level 1                       → 4 KiB leaf
        //   level 2 + HUGE_PAGE           → 2 MiB leaf
        //   level 3 + HUGE_PAGE           → 1 GiB leaf
        let is_leaf = level == 1
            || (level == 2 && flags.contains(PageTableFlags::HUGE_PAGE))
            || (level == 3 && flags.contains(PageTableFlags::HUGE_PAGE));

        if is_leaf {
            if flags.contains(PageTableFlags::WRITABLE)
                && !flags.contains(PageTableFlags::NO_EXECUTE)
            {
                violations += 1;
            }
        } else if let Ok(frame) = entry.frame() {
            let next_ptr = (phys_mem_offset + frame.start_address().as_u64()).as_ptr::<PageTable>();
            let next_table = unsafe { &*next_ptr };
            violations += count_wx_violations(next_table, level - 1, phys_mem_offset);
        }
    }

    violations
}

/// Check the W^X invariant across the **entire** address space reachable
/// from the current (or given) `OffsetPageTable`.
///
/// Returns `(passed, violation_count)` — if `passed` is `false` the caller
/// should log the count and may choose to panic or continue.
pub fn check_wx_invariant(mapper: &OffsetPageTable) -> (bool, usize) {
    let violations = count_wx_violations(mapper.level_4_table(), 4, mapper.phys_offset());
    (violations == 0, violations)
}

/// After loading an ELF segment, transition it from writable-only (W-)
/// to readable+executable (R-X) by clearing both `WRITABLE` and `NO_EXECUTE`.
///
/// This satisfies requirement 14.4: the segment is never W+X, even transiently,
/// because it was mapped with `NO_EXECUTE` during the data-copy phase.
///
/// Takes a raw `pml4_frame` to allow direct page-table walking without
/// depending on the `Mapper` update_flags API.
pub fn clear_write_and_allow_exec(
    pml4_frame: PhysFrame<Size4KiB>,
    phys_mem_offset: VirtAddr,
    virt_start: VirtAddr,
    size: u64,
) {
    let pml4_ptr =
        (phys_mem_offset + pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();
    let pml4 = unsafe { &mut *pml4_ptr };

    let end_addr = virt_start + size - 1u64;
    let pages = Page::<Size4KiB>::range_inclusive(
        Page::containing_address(virt_start),
        Page::containing_address(end_addr),
    );

    for page in pages {
        let vaddr = page.start_address();

        // Walk PML4 → PDPT → PD → PT
        let p4e = &pml4[vaddr.p4_index()];
        if p4e.is_unused() {
            continue;
        }
        let p3_ptr = (phys_mem_offset + p4e.frame().unwrap().start_address().as_u64())
            .as_mut_ptr::<PageTable>();
        let p3 = unsafe { &mut *p3_ptr };
        let p3e = &p3[vaddr.p3_index()];
        if p3e.is_unused() {
            continue;
        }
        let p2_ptr = (phys_mem_offset + p3e.frame().unwrap().start_address().as_u64())
            .as_mut_ptr::<PageTable>();
        let p2 = unsafe { &mut *p2_ptr };
        let p2e = &p2[vaddr.p2_index()];
        if p2e.is_unused() {
            continue;
        }

        // HUGE_PAGE at PD level → 2 MiB page, skip (not handled at this level)
        if p2e.flags().contains(PageTableFlags::HUGE_PAGE) {
            continue;
        }

        let p1_ptr = (phys_mem_offset + p2e.frame().unwrap().start_address().as_u64())
            .as_mut_ptr::<PageTable>();
        let p1 = unsafe { &mut *p1_ptr };
        let p1e = &mut p1[vaddr.p1_index()];

        let mut new_flags = p1e.flags();
        new_flags.remove(PageTableFlags::WRITABLE);
        new_flags.remove(PageTableFlags::NO_EXECUTE);
        p1e.set_flags(new_flags);
        tlb::flush(vaddr);
    }
}

/// Run the W^X self-check on the **kernel** address space and log the result.
/// Called once during early boot (after paging is initialised).
pub fn boot_self_check(phys_mem_offset: VirtAddr) -> usize {
    let (pml4_frame, _) = Cr3::read();
    let pml4_ptr =
        (phys_mem_offset + pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();
    let mapper = unsafe { OffsetPageTable::new(&mut *pml4_ptr, phys_mem_offset) };

    let (passed, count) = check_wx_invariant(&mapper);
    if passed {
        log::info!("W^X self-check: PASSED (0 violations)");
    } else {
        log::warn!("W^X self-check: FAILED ({} violations)", count);
    }
    count
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Generate random `PageTableFlags` that are a plausible subset of real
    /// x86_64 page-table flags (lower 12 bits + bit 63 for NO_EXECUTE).
    fn arb_pte_flags() -> impl Strategy<Value = PageTableFlags> {
        any::<u16>().prop_map(|bits| {
            let mut f = PageTableFlags::from_bits_truncate((bits & 0x0FFF) as u64);
            if bits & 0x8000 != 0 {
                f |= PageTableFlags::NO_EXECUTE;
            }
            f
        })
    }

    proptest! {
        /// Property 12 — W^X Invariant
        ///
        /// For any combination of page-table entry flags, the
        /// `enforce_wx_on_flags` function must ensure the resulting flags
        /// never have both `WRITABLE` and executable (no `NO_EXECUTE`) set.
        #[test]
        fn wx_invariant_property(flags in arb_pte_flags()) {
            let original = flags;
            let mut enforced = flags;
            let was_violation = enforce_wx_on_flags(&mut enforced);

            let is_wx_before =
                original.contains(PageTableFlags::WRITABLE)
                && !original.contains(PageTableFlags::NO_EXECUTE);

            let is_wx_after =
                enforced.contains(PageTableFlags::WRITABLE)
                && !enforced.contains(PageTableFlags::NO_EXECUTE);

            // After enforcement the invariant must hold
            prop_assert!(
                !is_wx_after,
                "enforce_wx_on_flags left a W+X entry: {:#?}",
                enforced,
            );

            // Return value must match the pre-enforcement state
            prop_assert_eq!(
                was_violation, is_wx_before,
                "enforce_wx_on_flags return value mismatch",
            );
        }
    }

    #[test]
    fn enforce_wx_leaves_non_wx_untouched() {
        let cases = [
            (PageTableFlags::PRESENT, PageTableFlags::PRESENT),
            (
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE,
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE,
            ),
            (
                PageTableFlags::PRESENT | PageTableFlags::NO_EXECUTE,
                PageTableFlags::PRESENT | PageTableFlags::NO_EXECUTE,
            ),
            (
                PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE,
                PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE,
            ),
        ];

        for (input, expected) in &cases {
            let mut f = *input;
            enforce_wx_on_flags(&mut f);
            assert_eq!(f, *expected, "non-W+X flags must not be changed");
        }
    }

    #[test]
    fn enforce_wx_strips_writable_from_wx() {
        let mut f =
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
        let was = enforce_wx_on_flags(&mut f);
        assert!(was, "W+X violation must be detected");
        assert!(
            !f.contains(PageTableFlags::WRITABLE),
            "WRITABLE must be stripped"
        );
        assert!(f.contains(PageTableFlags::PRESENT));
        assert!(f.contains(PageTableFlags::USER_ACCESSIBLE));
    }

    #[test]
    fn enforce_wx_ignores_nx() {
        let mut f = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
        let was = enforce_wx_on_flags(&mut f);
        assert!(!was, "W+NX is not a violation");
        assert!(f.contains(PageTableFlags::WRITABLE));
    }

    #[test]
    fn enforce_wx_huge_page_flag_untouched() {
        let mut f = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::HUGE_PAGE;
        let was = enforce_wx_on_flags(&mut f);
        assert!(was, "W+X with HUGE_PAGE must be detected");
        assert!(!f.contains(PageTableFlags::WRITABLE), "WRITABLE must be stripped");
        assert!(f.contains(PageTableFlags::HUGE_PAGE), "HUGE_PAGE must be preserved");
    }

    #[test]
    fn enforce_wx_global_flag_untouched() {
        let mut f = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::GLOBAL;
        let was = enforce_wx_on_flags(&mut f);
        assert!(was, "W+X with GLOBAL must be detected");
        assert!(!f.contains(PageTableFlags::WRITABLE));
        assert!(f.contains(PageTableFlags::GLOBAL), "GLOBAL must be preserved");
    }

    #[test]
    fn enforce_wx_all_flags_zero() {
        let mut f = PageTableFlags::empty();
        let was = enforce_wx_on_flags(&mut f);
        assert!(!was, "empty flags are never a violation");
        assert_eq!(f, PageTableFlags::empty());
    }

    #[test]
    fn enforce_wx_access_bits_preserved() {
        let mut f = PageTableFlags::PRESENT
            | PageTableFlags::WRITABLE
            | PageTableFlags::USER_ACCESSIBLE
            | PageTableFlags::ACCESSED
            | PageTableFlags::DIRTY;
        let was = enforce_wx_on_flags(&mut f);
        assert!(was);
        assert!(!f.contains(PageTableFlags::WRITABLE));
        assert!(f.contains(PageTableFlags::USER_ACCESSIBLE));
        assert!(f.contains(PageTableFlags::ACCESSED));
        assert!(f.contains(PageTableFlags::DIRTY));
    }

    #[test]
    fn enforce_wx_no_exec_flag_allows_writable() {
        let mut f = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
        let was = enforce_wx_on_flags(&mut f);
        assert!(!was, "W+NX is not a violation");
        assert!(f.contains(PageTableFlags::WRITABLE));
        assert!(f.contains(PageTableFlags::NO_EXECUTE));
    }

    #[test]
    fn enforce_wx_read_only_no_change() {
        let mut f = PageTableFlags::PRESENT | PageTableFlags::NO_EXECUTE;
        let was = enforce_wx_on_flags(&mut f);
        assert!(!was);
        assert!(f.contains(PageTableFlags::PRESENT));
    }
}
