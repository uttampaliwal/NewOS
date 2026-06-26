//! Transparent Huge Pages (THP)
//!
//! Promotes contiguous 4 KiB pages to 2 MiB huge pages when access patterns
//! indicate it would reduce TLB pressure. Regions are tracked individually
//! and promoted or demoted based on access frequency.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

/// Number of contiguous 4 KiB pages required to form a 2 MiB huge page.
pub const THP_PROMOTION_THRESHOLD: u32 = 512;

/// Number of ticks between automatic demotion scans.
pub const THP_DEMOTION_INTERVAL: u64 = 1000;

/// Maximum number of regions that may be deferred (awaiting promotion).
pub const THP_MAX_DEFERRED: usize = 64;

/// The state of a THP-tracked region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThpState {
    /// Not yet considered for promotion.
    None,
    /// Eligible for promotion; access count below threshold.
    Candidate,
    /// Currently backed by a 2 MiB huge page.
    Promoted,
    /// Was promoted but demoted back to base pages.
    Deferred,
}

impl Default for ThpState {
    fn default() -> Self {
        ThpState::None
    }
}

/// A single THP-tracked virtual memory region.
#[derive(Debug, Clone)]
pub struct ThpRegion {
    /// Base virtual address of the region.
    pub base_virt: u64,
    /// Size of the region in bytes.
    pub size_bytes: u64,
    /// Current promotion state.
    pub state: ThpState,
    /// Physical address backing this region when promoted, if assigned.
    pub backing_phys: Option<u64>,
    /// Number of recorded accesses to this region.
    pub access_count: u32,
    /// Tick count at which the last access was recorded.
    pub last_access_tick: u64,
}

impl ThpRegion {
    /// Create a new region in the `Candidate` state.
    fn new(base_virt: u64, size_bytes: u64) -> Self {
        ThpRegion {
            base_virt,
            size_bytes,
            state: ThpState::Candidate,
            backing_phys: None,
            access_count: 0,
            last_access_tick: 0,
        }
    }
}

/// Snapshot of THP subsystem statistics.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThpStats {
    /// Total number of regions promoted to huge pages.
    pub promotion_count: u64,
    /// Total number of regions demoted back to base pages.
    pub demotion_count: u64,
    /// Total number of huge pages split back into base pages.
    pub split_count: u64,
    /// Current number of tracked regions.
    pub total_regions: usize,
    /// Number of regions in the Deferred state.
    pub deferred_regions: usize,
}

/// The THP manager that tracks regions and drives promotion/demotion.
pub struct ThpManager {
    /// Virtual base address -> region descriptor.
    regions: BTreeMap<u64, ThpRegion>,
    /// Lifetime count of successful promotions.
    promotion_count: u64,
    /// Lifetime count of successful demotions.
    demotion_count: u64,
    /// Lifetime count of huge-page splits.
    split_count: u64,
    /// Monotonic tick counter used for demotion interval checks.
    scan_tick: AtomicU64,
    /// Whether the THP subsystem is active.
    enabled: bool,
    /// Hard cap on the number of tracked regions.
    max_regions: usize,
}

impl ThpManager {
    /// Create a new manager with sensible defaults.
    const fn new() -> Self {
        ThpManager {
            regions: BTreeMap::new(),
            promotion_count: 0,
            demotion_count: 0,
            split_count: 0,
            scan_tick: AtomicU64::new(0),
            enabled: false,
            max_regions: 256,
        }
    }

    /// Enable or disable the THP subsystem.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check whether THP is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Register a virtual memory region for THP tracking.
    ///
    /// Returns `true` if the region was successfully registered, `false` if
    /// the region limit has been reached or a region already exists at the
    /// given address.
    pub fn register_region(&mut self, base_virt: u64, size_bytes: u64) -> bool {
        if self.regions.len() >= self.max_regions {
            return false;
        }
        if self.regions.contains_key(&base_virt) {
            return false;
        }
        self.regions.insert(base_virt, ThpRegion::new(base_virt, size_bytes));
        true
    }

    /// Unregister a previously registered region.
    ///
    /// Returns `true` if the region was found and removed.
    pub fn unregister_region(&mut self, base_virt: u64) -> bool {
        self.regions.remove(&base_virt).is_some()
    }

    /// Promote a region to use a 2 MiB huge page.
    ///
    /// Only regions in the `Candidate` state can be promoted.
    /// Returns `true` on success.
    pub fn promote_region(&mut self, base_virt: u64, phys_addr: u64) -> bool {
        if let Some(region) = self.regions.get_mut(&base_virt) {
            if region.state == ThpState::Candidate {
                region.state = ThpState::Promoted;
                region.backing_phys = Some(phys_addr);
                self.promotion_count += 1;
                return true;
            }
        }
        false
    }

    /// Demote a region back to base pages.
    ///
    /// Only regions in the `Promoted` state can be demoted.
    /// Returns `true` on success.
    pub fn demote_region(&mut self, base_virt: u64) -> bool {
        if let Some(region) = self.regions.get_mut(&base_virt) {
            if region.state == ThpState::Promoted {
                region.state = ThpState::Deferred;
                region.backing_phys = None;
                self.demotion_count += 1;
                self.split_count += 1;
                return true;
            }
        }
        false
    }

    /// Record an access to the given region, incrementing its access count.
    pub fn record_access(&mut self, base_virt: u64, tick: u64) {
        if let Some(region) = self.regions.get_mut(&base_virt) {
            region.access_count = region.access_count.saturating_add(1);
            region.last_access_tick = tick;
        }
    }

    /// Scan all candidate regions and promote those that have reached the
    /// access threshold.
    ///
    /// Returns the number of regions promoted during this scan.
    pub fn scan_and_promote(&mut self) -> u32 {
        let tick = self.scan_tick.fetch_add(1, Ordering::Relaxed);
        let mut promoted = 0u32;
        let candidates: Vec<u64> = self
            .regions
            .iter()
            .filter(|(_, r)| r.state == ThpState::Candidate && r.access_count >= THP_PROMOTION_THRESHOLD)
            .map(|(&addr, _)| addr)
            .collect();

        for addr in candidates {
            if let Some(region) = self.regions.get_mut(&addr) {
                if region.state == ThpState::Candidate && region.access_count >= THP_PROMOTION_THRESHOLD {
                    // Use the virtual address as a placeholder physical address.
                    // In a real implementation the page allocator would provide
                    // a physically contiguous 2 MiB frame.
                    let phys = region.base_virt;
                    region.state = ThpState::Promoted;
                    region.backing_phys = Some(phys);
                    self.promotion_count += 1;
                    promoted += 1;
                }
            }
        }

        let _ = tick;
        promoted
    }

    /// Return a snapshot of the current statistics.
    pub fn stats(&self) -> ThpStats {
        ThpStats {
            promotion_count: self.promotion_count,
            demotion_count: self.demotion_count,
            split_count: self.split_count,
            total_regions: self.regions.len(),
            deferred_regions: self.deferred_count(),
        }
    }

    /// Return the number of tracked regions.
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// Return the number of regions in the `Deferred` state.
    pub fn deferred_count(&self) -> usize {
        self.regions.values().filter(|r| r.state == ThpState::Deferred).count()
    }
}

/// Global THP manager.
static THP: Mutex<ThpManager> = Mutex::new(ThpManager::new());

/// Initialise the THP subsystem.  Called once during kernel startup.
pub fn thp_init() {
    THP.lock().set_enabled(true);
    crate::serial::println!("[THP] Transparent Huge Pages subsystem initialised");
}

/// Enable or disable the THP subsystem globally.
pub fn thp_set_enabled(enabled: bool) {
    THP.lock().set_enabled(enabled);
}

/// Check whether the THP subsystem is enabled.
pub fn thp_is_enabled() -> bool {
    THP.lock().is_enabled()
}

/// Register a virtual memory region for THP tracking.
pub fn thp_register_region(base_virt: u64, size_bytes: u64) -> bool {
    THP.lock().register_region(base_virt, size_bytes)
}

/// Unregister a virtual memory region from THP tracking.
pub fn thp_unregister_region(base_virt: u64) -> bool {
    THP.lock().unregister_region(base_virt)
}

/// Promote a region to use a 2 MiB huge page.
pub fn thp_promote(base_virt: u64, phys_addr: u64) -> bool {
    THP.lock().promote_region(base_virt, phys_addr)
}

/// Demote a region back to base pages.
pub fn thp_demote(base_virt: u64) -> bool {
    THP.lock().demote_region(base_virt)
}

/// Record an access to a THP-tracked region.
pub fn thp_record_access(base_virt: u64, tick: u64) {
    THP.lock().record_access(base_virt, tick)
}

/// Scan candidate regions and promote those that exceed the access threshold.
pub fn thp_scan() -> u32 {
    THP.lock().scan_and_promote()
}

/// Return a snapshot of THP statistics.
pub fn thp_stats() -> ThpStats {
    THP.lock().stats()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thp_constants() {
        let _guard = crate::test_serial::acquire();
        assert_eq!(THP_PROMOTION_THRESHOLD, 512);
        assert_eq!(THP_DEMOTION_INTERVAL, 1000);
        assert_eq!(THP_MAX_DEFERRED, 64);
    }

    #[test]
    fn test_thp_state_default_is_none() {
        let _guard = crate::test_serial::acquire();
        assert_eq!(ThpState::default(), ThpState::None);
    }

    #[test]
    fn test_thp_manager_new_is_empty() {
        let _guard = crate::test_serial::acquire();
        let mgr = ThpManager::new();
        assert!(!mgr.is_enabled());
        assert_eq!(mgr.region_count(), 0);
        assert_eq!(mgr.deferred_count(), 0);
    }

    #[test]
    fn test_register_and_unregister_region() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = ThpManager::new();
        assert!(mgr.register_region(0x1000_0000, 0x200000));
        assert_eq!(mgr.region_count(), 1);
        assert!(mgr.unregister_region(0x1000_0000));
        assert_eq!(mgr.region_count(), 0);
        assert!(!mgr.unregister_region(0x1000_0000));
    }

    #[test]
    fn test_register_duplicate_region_fails() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = ThpManager::new();
        assert!(mgr.register_region(0x1000_0000, 0x200000));
        assert!(!mgr.register_region(0x1000_0000, 0x200000));
        assert_eq!(mgr.region_count(), 1);
    }

    #[test]
    fn test_register_region_respects_max() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = ThpManager::new();
        for i in 0..mgr.max_regions as u64 {
            assert!(mgr.register_region(i * 0x200000, 0x200000));
        }
        assert!(!mgr.register_region(0xFFFF_0000, 0x200000));
    }

    #[test]
    fn test_promote_region() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = ThpManager::new();
        mgr.register_region(0x1000_0000, 0x200000);
        assert!(mgr.promote_region(0x1000_0000, 0x4000_0000));
        let region = mgr.regions.get(&0x1000_0000).unwrap();
        assert_eq!(region.state, ThpState::Promoted);
        assert_eq!(region.backing_phys, Some(0x4000_0000));
        assert_eq!(mgr.promotion_count, 1);
    }

    #[test]
    fn test_promote_non_candidate_fails() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = ThpManager::new();
        mgr.register_region(0x1000_0000, 0x200000);
        mgr.promote_region(0x1000_0000, 0x4000_0000);
        // Already promoted — second promote should fail
        assert!(!mgr.promote_region(0x1000_0000, 0x5000_0000));
    }

    #[test]
    fn test_demote_region() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = ThpManager::new();
        mgr.register_region(0x1000_0000, 0x200000);
        mgr.promote_region(0x1000_0000, 0x4000_0000);
        assert!(mgr.demote_region(0x1000_0000));
        let region = mgr.regions.get(&0x1000_0000).unwrap();
        assert_eq!(region.state, ThpState::Deferred);
        assert!(region.backing_phys.is_none());
        assert_eq!(mgr.demotion_count, 1);
        assert_eq!(mgr.split_count, 1);
    }

    #[test]
    fn test_demote_non_promoted_fails() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = ThpManager::new();
        mgr.register_region(0x1000_0000, 0x200000);
        // Still a Candidate — cannot demote
        assert!(!mgr.demote_region(0x1000_0000));
    }

    #[test]
    fn test_record_access() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = ThpManager::new();
        mgr.register_region(0x1000_0000, 0x200000);
        mgr.record_access(0x1000_0000, 100);
        mgr.record_access(0x1000_0000, 200);
        let region = mgr.regions.get(&0x1000_0000).unwrap();
        assert_eq!(region.access_count, 2);
        assert_eq!(region.last_access_tick, 200);
    }

    #[test]
    fn test_scan_and_promote() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = ThpManager::new();
        mgr.register_region(0x1000_0000, 0x200000);
        mgr.register_region(0x2000_0000, 0x200000);

        // First region gets enough accesses to cross the threshold
        for i in 0..=THP_PROMOTION_THRESHOLD {
            mgr.record_access(0x1000_0000, i as u64);
        }
        // Second region gets only one access
        mgr.record_access(0x2000_0000, 1);

        let promoted = mgr.scan_and_promote();
        assert_eq!(promoted, 1);
        assert_eq!(mgr.regions[&0x1000_0000].state, ThpState::Promoted);
        assert_eq!(mgr.regions[&0x2000_0000].state, ThpState::Candidate);
    }

    #[test]
    fn test_stats() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = ThpManager::new();
        mgr.register_region(0x1000_0000, 0x200000);
        mgr.promote_region(0x1000_0000, 0x4000_0000);
        mgr.demote_region(0x1000_0000);
        let s = mgr.stats();
        assert_eq!(s.promotion_count, 1);
        assert_eq!(s.demotion_count, 1);
        assert_eq!(s.split_count, 1);
        assert_eq!(s.total_regions, 1);
        assert_eq!(s.deferred_regions, 1);
    }

    #[test]
    fn test_deferred_count() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = ThpManager::new();
        mgr.register_region(0x1000_0000, 0x200000);
        mgr.register_region(0x2000_0000, 0x200000);
        mgr.promote_region(0x1000_0000, 0x4000_0000);
        mgr.promote_region(0x2000_0000, 0x5000_0000);
        mgr.demote_region(0x1000_0000);
        assert_eq!(mgr.deferred_count(), 1);
        mgr.demote_region(0x2000_0000);
        assert_eq!(mgr.deferred_count(), 2);
    }

    #[test]
    fn test_global_api() {
        let _guard = crate::test_serial::acquire();
        thp_init();
        assert!(thp_is_enabled());

        assert!(thp_register_region(0x3000_0000, 0x200000));
        assert!(thp_promote(0x3000_0000, 0x6000_0000));
        assert!(thp_demote(0x3000_0000));

        let s = thp_stats();
        assert_eq!(s.promotion_count, 1);
        assert_eq!(s.demotion_count, 1);
        assert_eq!(s.total_regions, 1);

        assert!(thp_unregister_region(0x3000_0000));
        assert_eq!(thp_stats().total_regions, 0);

        thp_set_enabled(false);
        assert!(!thp_is_enabled());
    }
}
