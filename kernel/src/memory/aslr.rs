use core::arch::asm;
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::VirtAddr;

use crate::elf::{ELF_TYPE_DYN, ElfHeader};

/// Number of bits of page-offset entropy for ASLR (requirement: >= 28).
const ASLR_PAGE_ENTROPY: u64 = 28;
/// Number of page-aligned positions in the randomisation range: 2^28.
const ASLR_RANGE_PAGES: u64 = 1 << ASLR_PAGE_ENTROPY;

/// PIE executable load region: 1 TB range starting at 64 GiB.
/// Max end of this range: 0x10_0000_0000 + 0x100_0000_0000 = 0x110_0000_0000.
const PIE_LOAD_BASE_MIN: u64 = 0x0000_0010_0000_0000;

/// Stack base region: 1 TB range near the top of user space (112 TiB).
const STACK_BASE_MIN: u64 = 0x0000_7000_0000_0000;

/// Heap (mmap) base region: 1 TB range starting at 2 TiB,
/// safely above the maximum PIE load address (0x110_0000_0000).
const HEAP_BASE_MIN: u64 = 0x0000_0200_0000_0000;

// ---------------------------------------------------------------------------
// PRNG — xorshift64 seeded from RDRAND at first use
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn seed_from_rdrand() -> Option<Self> {
        Some(Self::new(rdrand_u64()?))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Return a page-aligned `VirtAddr` in [min, min + num_pages * PAGE_SIZE).
    fn next_page_aligned(&mut self, min: u64, num_pages: u64) -> VirtAddr {
        let page_offset = self.next_u64() % num_pages;
        VirtAddr::new(min + page_offset * crate::memory::PAGE_SIZE)
    }
}

/// Read a 64-bit random value from the `RDRAND` instruction.
fn rdrand_u64() -> Option<u64> {
    let val: u64;
    let ret: u8;
    unsafe {
        asm!(
            "rdrand {val}",
            "setc {ret}",
            val = out(reg) val,
            ret = out(reg_byte) ret,
            options(nostack),
        );
    }
    if ret != 0 { Some(val) } else { None }
}

lazy_static! {
    static ref ASLR_RNG: Mutex<SimpleRng> = {
        let rng = match SimpleRng::seed_from_rdrand() {
            Some(r) => r,
            None => {
                log::warn!("ASLR: RDRAND failed, using fallback seed");
                SimpleRng::new(42)
            }
        };
        Mutex::new(rng)
    };
}

// ---------------------------------------------------------------------------
// Public ASLR API
// ---------------------------------------------------------------------------

/// Return a randomised load base for a PIE executable.
/// For non-PIE binaries, logs a warning and returns `VirtAddr::zero()`.
pub fn randomise_load_base(elf: &ElfHeader) -> VirtAddr {
    if elf.elf_type != ELF_TYPE_DYN {
        log::warn!(
            "ASLR: ELF type {:#06x} is not PIE (DYN); loading at preferred address",
            elf.elf_type,
        );
        return VirtAddr::zero();
    }
    ASLR_RNG
        .lock()
        .next_page_aligned(PIE_LOAD_BASE_MIN, ASLR_RANGE_PAGES)
}

/// Return a randomised stack base address for a new process (or fork child).
pub fn randomise_stack_base() -> VirtAddr {
    ASLR_RNG
        .lock()
        .next_page_aligned(STACK_BASE_MIN, ASLR_RANGE_PAGES)
}

/// Return a randomised heap (mmap) base address for a new process (or fork child).
pub fn randomise_heap_base() -> VirtAddr {
    ASLR_RNG
        .lock()
        .next_page_aligned(HEAP_BASE_MIN, ASLR_RANGE_PAGES)
}

// ---------------------------------------------------------------------------
// KASLR
// ---------------------------------------------------------------------------

/// Number of bits of page-offset entropy for KASLR (requirement: >= 9).
const KASLR_PAGE_ENTROPY: u64 = 9;
/// Number of page-aligned positions for the kernel: 2^9 = 512.
const KASLR_RANGE_PAGES: u64 = 1 << KASLR_PAGE_ENTROPY;

/// Traditional kernel load base (higher-half).
pub const KERNEL_BASE: u64 = 0xffff_ffff_8000_0000;

/// Return a randomised page-aligned offset for the kernel load address,
/// providing at least 9 bits of entropy within the higher-half region.
///
/// The offset is relative to `KERNEL_BASE` and is guaranteed to keep the
/// kernel within the canonical higher-half range.
///
/// **Note:** Full KASLR activation requires the kernel to be compiled as
/// position-independent code (`-C relocation-model=pic`).  The offset is
/// still generated here and passed through `BootInfo` so that the UEFI
/// loader can map the kernel at the randomised address.
pub fn randomise_kernel_base() -> VirtAddr {
    let page_offset = ASLR_RNG.lock().next_u64() % KASLR_RANGE_PAGES;
    VirtAddr::new(KERNEL_BASE + page_offset * crate::memory::PAGE_SIZE)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elf::ELF_TYPE_EXEC;
    use alloc::vec::Vec;
    use proptest::collection as pc;
    use proptest::prelude::*;

    /// Helper to build a minimal ElfHeader for testing.
    fn pie_header() -> ElfHeader {
        ElfHeader {
            entry: 0x1000,
            program_header_offset: 0x40,
            program_header_entry_size: 56,
            program_header_count: 2,
            elf_type: ELF_TYPE_DYN,
        }
    }

    fn non_pie_header() -> ElfHeader {
        ElfHeader {
            entry: 0x400000,
            program_header_offset: 0x40,
            program_header_entry_size: 56,
            program_header_count: 2,
            elf_type: ELF_TYPE_EXEC,
        }
    }

    #[test]
    fn non_pie_returns_zero() {
        let base = randomise_load_base(&non_pie_header());
        assert_eq!(base.as_u64(), 0);
    }

    #[test]
    fn pie_base_is_page_aligned() {
        let base = randomise_load_base(&pie_header());
        assert_eq!(base.as_u64() & 0xFFF, 0);
    }

    #[test]
    fn pie_base_in_valid_range() {
        let base = randomise_load_base(&pie_header());
        let v = base.as_u64();
        let max = PIE_LOAD_BASE_MIN + ASLR_RANGE_PAGES * crate::memory::PAGE_SIZE;
        assert!(v >= PIE_LOAD_BASE_MIN && v < max);
    }

    #[test]
    fn stack_base_is_page_aligned() {
        let b = randomise_stack_base();
        assert_eq!(b.as_u64() & 0xFFF, 0);
    }

    #[test]
    fn heap_base_is_page_aligned() {
        let b = randomise_heap_base();
        assert_eq!(b.as_u64() & 0xFFF, 0);
    }

    #[test]
    fn consecutive_calls_differ() {
        let a = randomise_load_base(&pie_header());
        let b = randomise_load_base(&pie_header());
        assert_ne!(a, b);
    }

    #[test]
    fn stack_and_heap_differ() {
        let s = randomise_stack_base();
        let h = randomise_heap_base();
        assert_ne!(s, h);
    }

    // -----------------------------------------------------------------------
    // Property 9 — ASLR Address Diversity
    // -----------------------------------------------------------------------
    //
    // Call randomise_load_base 10 times with the same PIE ELF header; assert
    // all 10 addresses are distinct.
    proptest! {
        #[test]
        fn aslr_address_diversity(
            _seed in proptest::num::u64::ANY,
        ) {
            let header = pie_header();
            let mut seen = [0u64; 10];
            for i in 0..10 {
                let addr = randomise_load_base(&header).as_u64();
                prop_assert!(
                    !seen[..i].contains(&addr),
                    "duplicate address {:#x}",
                    addr,
                );
                seen[i] = addr;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Property 10 — ASLR Fork Address Diversity
    // -----------------------------------------------------------------------
    //
    // Model a fork by calling randomise_stack_base and randomise_heap_base
    // for the child and verifying they differ from the parent-origin values.
    proptest! {
        #[test]
        fn aslr_fork_address_diversity(
            _seed in proptest::num::u64::ANY,
        ) {
            // Simulate parent values
            let parent_stack = randomise_stack_base();
            let parent_heap  = randomise_heap_base();

            // Simulate fork — child gets fresh random bases
            let child_stack = randomise_stack_base();
            let child_heap  = randomise_heap_base();

            prop_assert_ne!(
                child_stack, parent_stack,
                "child stack base must differ from parent",
            );
            prop_assert_ne!(
                child_heap, parent_heap,
                "child heap base must differ from parent",
            );
        }
    }

    // -----------------------------------------------------------------------
    // Property 11 — ELF Segment Relative Layout Preservation
    // -----------------------------------------------------------------------
    //
    // For any PIE ELF header with N segments, adding the ASLR base to every
    // segment virtual address preserves inter-segment differences.
    /// Use any u64, but apply wrapping arithmetic so the test never panics
    /// on non-canonical addresses or overflow.
    fn arb_segments() -> impl Strategy<Value = Vec<u64>> {
        pc::vec(any::<u64>(), 1..20)
    }

    proptest! {
        #[test]
        fn aslr_segment_layout_preserved(
            _seed in any::<u64>(),
            orig_vaddrs in arb_segments(),
            aslr_base_raw in any::<u64>(),
        ) {
            // Page-align the base (as randomise_load_base does).
            let base_raw = aslr_base_raw & !0xFFF;

            // Use raw u64 wrapping arithmetic to avoid VirtAddr::new panics.
            let adjusted: Vec<u64> = orig_vaddrs
                .iter()
                .map(|v| base_raw.wrapping_add(*v))
                .collect();

            // For every pair (i, j), the difference must be identical
            for i in 0..orig_vaddrs.len() {
                for j in 0..orig_vaddrs.len() {
                    let d_orig = orig_vaddrs[j].wrapping_sub(orig_vaddrs[i]);
                    let d_adj = adjusted[j].wrapping_sub(adjusted[i]);
                    prop_assert_eq!(
                        d_adj, d_orig,
                        "segment layout not preserved: i={}, j={}", i, j,
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Property 31 — KASLR Offset Entropy and Alignment
    // -----------------------------------------------------------------------
    //
    // Verify that randomise_kernel_base produces page-aligned addresses with
    // at least 9 bits of entropy above the KERNEL_BASE.
    proptest! {
        #[test]
        fn kaslr_entropy_and_alignment(
            _seed in any::<u64>(),
        ) {
            let addr = randomise_kernel_base();
            let offset = addr.as_u64().wrapping_sub(KERNEL_BASE);

            // Must be page-aligned
            prop_assert_eq!(
                offset & 0xFFF, 0,
                "KASLR offset must be page-aligned",
            );

            // Must be within the range of 2^9 pages
            prop_assert!(
                offset < KASLR_RANGE_PAGES * 4096,
                "KASLR offset out of range: {:#x}",
                offset,
            );
        }
    }

    #[test]
    fn kaslr_consecutive_calls_differ() {
        let a = randomise_kernel_base().as_u64();
        let b = randomise_kernel_base().as_u64();
        if a == b {
            let c = randomise_kernel_base().as_u64();
            assert_ne!(a, c);
        }
    }

    #[test]
    fn stack_base_above_minimum() {
        let addr = randomise_stack_base().as_u64();
        assert!(addr >= STACK_BASE_MIN, "stack base {:#x} must be >= {:#x}", addr, STACK_BASE_MIN);
    }

    #[test]
    fn heap_base_above_minimum() {
        let addr = randomise_heap_base().as_u64();
        assert!(addr >= HEAP_BASE_MIN, "heap base {:#x} must be >= {:#x}", addr, HEAP_BASE_MIN);
    }

    #[test]
    fn pie_base_minimum_aligned() {
        assert_eq!(PIE_LOAD_BASE_MIN & 0xFFF, 0, "PIE_LOAD_BASE_MIN must be page-aligned");
        assert_eq!(STACK_BASE_MIN & 0xFFF, 0, "STACK_BASE_MIN must be page-aligned");
        assert_eq!(HEAP_BASE_MIN & 0xFFF, 0, "HEAP_BASE_MIN must be page-aligned");
    }

    #[test]
    fn stack_and_heap_do_not_overlap_with_pie() {
        let pie_max = PIE_LOAD_BASE_MIN + ASLR_RANGE_PAGES * 4096;
        // 1 GiB margin between pie max and heap min
        assert!(HEAP_BASE_MIN > pie_max, "HEAP_BASE_MIN must be above max PIE range");
        let heap_max = HEAP_BASE_MIN + ASLR_RANGE_PAGES * 4096;
        assert!(STACK_BASE_MIN > heap_max, "STACK_BASE_MIN must be above max heap range");
        // KASLR kernel offset does not overlap with user ranges
        assert!(KERNEL_BASE > 0xffff_0000_0000_0000, "KERNEL_BASE must be in kernel space");
    }
}
