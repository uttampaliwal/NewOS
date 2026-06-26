//! Huge Page Allocator
//!
//! Provides 2 MiB and 1 GiB huge page allocation for reduced TLB pressure.
//! Pages are allocated from a reserved pool and can be mapped into process
//! address spaces via the hugetlbfs interface.

use alloc::collections::BTreeMap;
use spin::Mutex;

/// Standard 4 KiB page size.
pub const PAGE_SIZE: u64 = 4096;

/// 2 MiB huge page size (512 × 4 KiB).
pub const HUGE_PAGE_2M: u64 = 2 * 1024 * 1024;

/// 1 GiB huge page size (512 × 2 MiB).
pub const HUGE_PAGE_1G: u64 = 1024 * 1024 * 1024;

/// Default pool sizes (used when auto-configuring).
pub const DEFAULT_HUGE_PAGE_POOL_2M: usize = 64;
/// Default pool sizes (used when auto-configuring).
pub const DEFAULT_HUGE_PAGE_POOL_1G: usize = 4;

/// A single huge page descriptor.
#[derive(Debug, Clone)]
pub struct HugePage {
    /// Physical base address (must be aligned to page size).
    pub phys_addr: u64,
    /// Size of this huge page (HUGE_PAGE_2M or HUGE_PAGE_1G).
    pub size: u64,
    /// Whether this page is currently allocated (in use).
    pub allocated: bool,
    /// Optional NUMA node affinity (-1 = any node).
    pub node: i32,
}

/// Statistics for the huge page subsystem.
#[derive(Debug, Clone, Copy, Default)]
pub struct HugePageStats {
    /// Total 2 MiB huge pages reserved.
    pub total_2m: u64,
    /// Free 2 MiB huge pages.
    pub free_2m: u64,
    /// Total 1 GiB huge pages reserved.
    pub total_1g: u64,
    /// Free 1 GiB huge pages.
    pub free_1g: u64,
    /// Number of successful 2 MiB allocations.
    pub alloc_2m: u64,
    /// Number of successful 1 GiB allocations.
    pub alloc_1g: u64,
    /// Number of allocation failures (pool exhausted).
    pub alloc_failures: u64,
}

/// The huge page pool manager.
pub struct HugePagePool {
    /// Pool of 2 MiB huge pages, keyed by physical address.
    pages_2m: BTreeMap<u64, HugePage>,
    /// Pool of 1 GiB huge pages, keyed by physical address.
    pages_1g: BTreeMap<u64, HugePage>,
    /// Allocation statistics.
    stats: HugePageStats,
    /// Base physical address for the 2 MiB pool (set during init).
    pool_base_2m: u64,
    /// Base physical address for the 1 GiB pool (set during init).
    pool_base_1g: u64,
    /// Whether the pool has been initialized.
    initialized: bool,
}

impl HugePagePool {
    const fn new() -> Self {
        HugePagePool {
            pages_2m: BTreeMap::new(),
            pages_1g: BTreeMap::new(),
            stats: HugePageStats {
                total_2m: 0,
                free_2m: 0,
                total_1g: 0,
                free_1g: 0,
                alloc_2m: 0,
                alloc_1g: 0,
                alloc_failures: 0,
            },
            pool_base_2m: 0,
            pool_base_1g: 0,
            initialized: false,
        }
    }

    /// Initialize the huge page pool with a contiguous physical region.
    ///
    /// # Safety
    ///
    /// `base_2m` must point to a physically contiguous region of at least
    /// `count_2m * HUGE_PAGE_2M` bytes that is not used by any other subsystem.
    /// Similarly for `base_1g`.
    pub unsafe fn init(&mut self, base_2m: u64, count_2m: usize, base_1g: u64, count_1g: usize) {
        self.pool_base_2m = base_2m;
        self.pool_base_1g = base_1g;

        for i in 0..count_2m {
            let phys = base_2m + (i as u64) * HUGE_PAGE_2M;
            self.pages_2m.insert(
                phys,
                HugePage {
                    phys_addr: phys,
                    size: HUGE_PAGE_2M,
                    allocated: false,
                    node: -1,
                },
            );
        }
        self.stats.total_2m = count_2m as u64;
        self.stats.free_2m = count_2m as u64;

        for i in 0..count_1g {
            let phys = base_1g + (i as u64) * HUGE_PAGE_1G;
            self.pages_1g.insert(
                phys,
                HugePage {
                    phys_addr: phys,
                    size: HUGE_PAGE_1G,
                    allocated: false,
                    node: -1,
                },
            );
        }
        self.stats.total_1g = count_1g as u64;
        self.stats.free_1g = count_1g as u64;

        self.initialized = true;
        crate::serial::println!(
            "[HUGEPAGE] Pool initialized: {} x 2MiB, {} x 1GiB",
            count_2m,
            count_1g
        );
    }

    /// Allocate a 2 MiB huge page.
    pub fn alloc_2m(&mut self) -> Option<u64> {
        // Find the first free 2 MiB page
        let phys = self
            .pages_2m
            .values_mut()
            .find(|p| !p.allocated)
            .map(|p| {
                p.allocated = true;
                p.phys_addr
            });

        if phys.is_some() {
            self.stats.alloc_2m += 1;
            self.stats.free_2m -= 1;
        } else {
            self.stats.alloc_failures += 1;
        }

        phys
    }

    /// Allocate a 1 GiB huge page.
    pub fn alloc_1g(&mut self) -> Option<u64> {
        let phys = self
            .pages_1g
            .values_mut()
            .find(|p| !p.allocated)
            .map(|p| {
                p.allocated = true;
                p.phys_addr
            });

        if phys.is_some() {
            self.stats.alloc_1g += 1;
            self.stats.free_1g -= 1;
        } else {
            self.stats.alloc_failures += 1;
        }

        phys
    }

    /// Free a previously allocated huge page.
    pub fn free(&mut self, phys_addr: u64) -> bool {
        if let Some(page) = self.pages_2m.get_mut(&phys_addr) {
            if page.allocated {
                page.allocated = false;
                self.stats.free_2m += 1;
                return true;
            }
        }
        if let Some(page) = self.pages_1g.get_mut(&phys_addr) {
            if page.allocated {
                page.allocated = false;
                self.stats.free_1g += 1;
                return true;
            }
        }
        false
    }

    /// Get current statistics.
    pub fn stats(&self) -> HugePageStats {
        self.stats
    }

    /// Check if a physical address is a huge page base address.
    pub fn is_huge_page(&self, phys_addr: u64) -> bool {
        self.pages_2m.contains_key(&phys_addr) || self.pages_1g.contains_key(&phys_addr)
    }

    /// Get the number of free huge pages of each size.
    pub fn free_counts(&self) -> (u64, u64) {
        (self.stats.free_2m, self.stats.free_1g)
    }
}

/// Global huge page pool.
static HUGE_PAGE_POOL: Mutex<HugePagePool> = Mutex::new(HugePagePool::new());

/// Initialize the huge page pool. Called during early boot after heap is ready.
///
/// # Safety
///
/// `base_2m` must point to a physically contiguous region not used by other subsystems.
pub unsafe fn init(base_2m: u64, count_2m: usize, base_1g: u64, count_1g: usize) {
    unsafe {
        HUGE_PAGE_POOL
            .lock()
            .init(base_2m, count_2m, base_1g, count_1g);
    }
}

/// Allocate a 2 MiB huge page. Returns the physical address.
pub fn alloc_huge_page_2m() -> Option<u64> {
    HUGE_PAGE_POOL.lock().alloc_2m()
}

/// Allocate a 1 GiB huge page. Returns the physical address.
pub fn alloc_huge_page_1g() -> Option<u64> {
    HUGE_PAGE_POOL.lock().alloc_1g()
}

/// Free a huge page by physical address.
pub fn free_huge_page(phys_addr: u64) -> bool {
    HUGE_PAGE_POOL.lock().free(phys_addr)
}

/// Get huge page statistics.
pub fn huge_page_stats() -> HugePageStats {
    HUGE_PAGE_POOL.lock().stats()
}

/// Check if a physical address belongs to a huge page.
pub fn is_huge_page_addr(phys_addr: u64) -> bool {
    HUGE_PAGE_POOL.lock().is_huge_page(phys_addr)
}

/// Get the count of free huge pages (2m, 1g).
pub fn free_huge_page_counts() -> (u64, u64) {
    HUGE_PAGE_POOL.lock().free_counts()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_huge_page_constants() {
        let _guard = crate::test_serial::acquire();
        assert_eq!(HUGE_PAGE_2M, 2 * 1024 * 1024);
        assert_eq!(HUGE_PAGE_1G, 1024 * 1024 * 1024);
        assert_eq!(HUGE_PAGE_2M / PAGE_SIZE, 512);
        assert_eq!(HUGE_PAGE_1G / HUGE_PAGE_2M, 512);
    }

    #[test]
    fn test_huge_page_pool_new_is_empty() {
        let _guard = crate::test_serial::acquire();
        let pool = HugePagePool::new();
        assert!(!pool.initialized);
        assert_eq!(pool.stats.total_2m, 0);
        assert_eq!(pool.stats.total_1g, 0);
    }

    #[test]
    fn test_huge_page_pool_init() {
        let _guard = crate::test_serial::acquire();
        let mut pool = HugePagePool::new();
        // Safety: using fake addresses for testing only
        unsafe {
            pool.init(0x10000000, 8, 0x20000000, 2);
        }
        assert!(pool.initialized);
        assert_eq!(pool.stats.total_2m, 8);
        assert_eq!(pool.stats.free_2m, 8);
        assert_eq!(pool.stats.total_1g, 2);
        assert_eq!(pool.stats.free_1g, 2);
    }

    #[test]
    fn test_alloc_2m_returns_contiguous_addresses() {
        let _guard = crate::test_serial::acquire();
        let mut pool = HugePagePool::new();
        unsafe {
            pool.init(0x10000000, 4, 0x20000000, 1);
        }
        let addr1 = pool.alloc_2m().unwrap();
        let addr2 = pool.alloc_2m().unwrap();
        assert_eq!(addr2 - addr1, HUGE_PAGE_2M, "consecutive allocations should be contiguous");
        assert_eq!(pool.stats.free_2m, 2);
    }

    #[test]
    fn test_alloc_2m_exhaustion() {
        let _guard = crate::test_serial::acquire();
        let mut pool = HugePagePool::new();
        unsafe {
            pool.init(0x10000000, 2, 0x20000000, 0);
        }
        assert!(pool.alloc_2m().is_some());
        assert!(pool.alloc_2m().is_some());
        assert!(pool.alloc_2m().is_none(), "should fail when pool exhausted");
        assert_eq!(pool.stats.alloc_failures, 1);
    }

    #[test]
    fn test_free_2m_reclaim() {
        let _guard = crate::test_serial::acquire();
        let mut pool = HugePagePool::new();
        unsafe {
            pool.init(0x10000000, 2, 0x20000000, 0);
        }
        let addr = pool.alloc_2m().unwrap();
        assert_eq!(pool.stats.free_2m, 1);
        assert!(pool.free(addr));
        assert_eq!(pool.stats.free_2m, 2);
        // Should be able to allocate again
        assert!(pool.alloc_2m().is_some());
    }

    #[test]
    fn test_free_invalid_address_returns_false() {
        let _guard = crate::test_serial::acquire();
        let mut pool = HugePagePool::new();
        unsafe {
            pool.init(0x10000000, 2, 0x20000000, 0);
        }
        assert!(!pool.free(0xDEAD), "freeing invalid address should return false");
    }

    #[test]
    fn test_alloc_1g() {
        let _guard = crate::test_serial::acquire();
        let mut pool = HugePagePool::new();
        unsafe {
            pool.init(0x10000000, 0, 0x20000000, 2);
        }
        let addr = pool.alloc_1g().unwrap();
        assert_eq!(addr, 0x20000000);
        assert_eq!(pool.stats.free_1g, 1);
        assert!(pool.free(addr));
        assert_eq!(pool.stats.free_1g, 2);
    }

    #[test]
    fn test_is_huge_page() {
        let _guard = crate::test_serial::acquire();
        let mut pool = HugePagePool::new();
        unsafe {
            pool.init(0x10000000, 2, 0x20000000, 1);
        }
        assert!(pool.is_huge_page(0x10000000));
        assert!(pool.is_huge_page(0x10000000 + HUGE_PAGE_2M));
        assert!(pool.is_huge_page(0x20000000));
        assert!(!pool.is_huge_page(0x30000000));
    }

    #[test]
    fn test_stats_tracking() {
        let _guard = crate::test_serial::acquire();
        let mut pool = HugePagePool::new();
        unsafe {
            pool.init(0x10000000, 3, 0x20000000, 1);
        }
        let a1 = pool.alloc_2m().unwrap();
        let _a2 = pool.alloc_2m().unwrap();
        assert_eq!(pool.stats.alloc_2m, 2);
        pool.free(a1);
        assert_eq!(pool.stats.free_2m, 2); // 1 original free + 1 freed
        pool.alloc_2m(); // re-alloc
        assert_eq!(pool.stats.alloc_2m, 3);
    }

    #[test]
    fn test_double_free_returns_false() {
        let _guard = crate::test_serial::acquire();
        let mut pool = HugePagePool::new();
        unsafe {
            pool.init(0x10000000, 1, 0x20000000, 0);
        }
        let addr = pool.alloc_2m().unwrap();
        assert!(pool.free(addr));
        assert!(!pool.free(addr), "double free should return false");
    }

    #[test]
    fn test_free_counts_consistency() {
        let _guard = crate::test_serial::acquire();
        let mut pool = HugePagePool::new();
        unsafe {
            pool.init(0x10000000, 4, 0x20000000, 2);
        }
        let (f2m, f1g) = pool.free_counts();
        assert_eq!(f2m, 4);
        assert_eq!(f1g, 2);
        pool.alloc_2m();
        pool.alloc_1g();
        let (f2m, f1g) = pool.free_counts();
        assert_eq!(f2m, 3);
        assert_eq!(f1g, 1);
    }
}
