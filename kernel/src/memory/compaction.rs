//! Memory Compaction
//!
//! Scans zones and migrates pages to create contiguous free runs for
//! high-order allocations (e.g. 2 MiB huge pages). Works by identifying
//! movable pages, compacting them toward one end of the zone, and
//! coalescing the resulting free runs.

use spin::Mutex;

/// Compaction zone descriptor.
#[derive(Debug, Clone)]
pub struct CompactionZone {
    pub name: &'static str,
    pub start_pfn: u64,
    pub end_pfn: u64,
    pub free_pfn_count: u64,
    pub contiguous_required: usize,
}

/// Result of a compaction attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionResult {
    /// Contiguous run found at the given start PFN.
    Success(u64),
    /// Compaction could not satisfy the request.
    Failed,
    /// Partially succeeded: pages migrated but target not reached.
    Partial(usize),
}

/// Lifetime statistics for compaction operations.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompactionStats {
    pub migrated: u64,
    pub failed: u64,
    pub scanned: u64,
}

/// Wraps a zone with mutable compaction state.
#[derive(Debug)]
pub struct CompactZone {
    pub zone: CompactionZone,
    pub stats: CompactionStats,
}

/// Static compaction statistics accumulator.
static COMPACT_STATS: Mutex<CompactionStats> = Mutex::new(CompactionStats {
    migrated: 0,
    failed: 0,
    scanned: 0,
});

/// Scan the zone and migrate movable pages to create contiguous free runs.
///
/// The algorithm walks the zone in page-frame-number order. It tracks the
/// head of a potential free run; when it encounters a movable page it
/// "migrates" it (simulated by bumping a counter) and extends the run.
/// If a run of `zone.contiguous_required` pages is found, the start PFN
/// is returned in `CompactionResult::Success`.
pub fn compact_zone(zone: &mut CompactZone) -> CompactionResult {
    let order = zone.zone.contiguous_required;
    let total_pages = (zone.zone.end_pfn - zone.zone.start_pfn) as usize;
    if total_pages == 0 || order == 0 {
        return CompactionResult::Failed;
    }

    let mut free_run: u64 = 0;
    let mut run_start: u64 = zone.zone.start_pfn;
    let mut migrated: u64 = 0;
    let mut scanned: u64 = 0;

    for pfn in zone.zone.start_pfn..zone.zone.end_pfn {
        scanned += 1;

        // Simulate page state: pages at even PFNs are "free", odd are "movable".
        let is_free = pfn % 2 == 0;
        let is_movable = pfn % 2 == 1;

        if is_free {
            if free_run == 0 {
                run_start = pfn;
            }
            free_run += 1;
            if free_run >= order as u64 {
                zone.stats.migrated += migrated;
                zone.stats.scanned += scanned;
                merge_stats(migrated, 0, scanned);
                return CompactionResult::Success(run_start);
            }
        } else if is_movable {
            migrated += 1;
            // After "migration" this page becomes free, extending the run.
            if free_run == 0 {
                run_start = pfn;
            }
            free_run += 1;
            if free_run >= order as u64 {
                zone.stats.migrated += migrated;
                zone.stats.scanned += scanned;
                merge_stats(migrated, 0, scanned);
                return CompactionResult::Success(run_start);
            }
        } else {
            // Unmovable page — break the free run.
            free_run = 0;
        }
    }

    zone.stats.migrated += migrated;
    zone.stats.scanned += scanned;
    if migrated > 0 {
        merge_stats(migrated, 0, scanned);
        CompactionResult::Partial(migrated as usize)
    } else {
        merge_stats(0, 1, scanned);
        CompactionResult::Failed
    }
}

fn merge_stats(migrated: u64, failed: u64, scanned: u64) {
    let mut s = COMPACT_STATS.lock();
    s.migrated += migrated;
    s.failed += failed;
    s.scanned += scanned;
}

/// Try to compact for a given order (0 = 4 KiB, 1 = 8 KiB, ..., 9 = 2 MiB).
///
/// Returns the start PFN of the contiguous run on success.
pub fn compact_order(order: u8) -> Option<u64> {
    let pages_required = 1usize << order;
    let mut zone = CompactZone {
        zone: CompactionZone {
            name: "DMA",
            start_pfn: 0x1000,
            end_pfn: 0x2000,
            free_pfn_count: 256,
            contiguous_required: pages_required,
        },
        stats: CompactionStats::default(),
    };

    match compact_zone(&mut zone) {
        CompactionResult::Success(pfn) => Some(pfn),
        _ => None,
    }
}

/// Lightweight defragmentation pass — compacts order-0 (single page) runs.
pub fn defrag_zone(zone: &mut CompactZone) {
    zone.zone.contiguous_required = 1;
    let _ = compact_zone(zone);
}

/// Return a snapshot of the global compaction statistics.
pub fn compaction_stats() -> CompactionStats {
    let s = COMPACT_STATS.lock();
    CompactionStats {
        migrated: s.migrated,
        failed: s.failed,
        scanned: s.scanned,
    }
}

/// Reset global compaction statistics to zero.
pub fn reset_compaction_stats() {
    let mut s = COMPACT_STATS.lock();
    s.migrated = 0;
    s.failed = 0;
    s.scanned = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compaction_result_variants() {
        let _guard = crate::test_serial::acquire();
        assert_eq!(CompactionResult::Success(0), CompactionResult::Success(0));
        assert_eq!(CompactionResult::Failed, CompactionResult::Failed);
        assert_ne!(CompactionResult::Failed, CompactionResult::Partial(1));
    }

    #[test]
    fn test_compact_zone_order_1_succeeds() {
        let _guard = crate::test_serial::acquire();
        let mut zone = CompactZone {
            zone: CompactionZone {
                name: "test",
                start_pfn: 0x100,
                end_pfn: 0x200,
                free_pfn_count: 128,
                contiguous_required: 1,
            },
            stats: CompactionStats::default(),
        };
        let result = compact_zone(&mut zone);
        assert!(matches!(result, CompactionResult::Success(_)));
    }

    #[test]
    fn test_compact_zone_partial_result() {
        let _guard = crate::test_serial::acquire();
        let mut zone = CompactZone {
            zone: CompactionZone {
                name: "test",
                start_pfn: 0x100,
                end_pfn: 0x104,
                free_pfn_count: 2,
                contiguous_required: 1024,
            },
            stats: CompactionStats::default(),
        };
        let result = compact_zone(&mut zone);
        assert!(
            matches!(result, CompactionResult::Partial(_)),
            "expected Partial, got {:?}",
            result
        );
    }

    #[test]
    fn test_compact_zone_zero_pages() {
        let _guard = crate::test_serial::acquire();
        let mut zone = CompactZone {
            zone: CompactionZone {
                name: "empty",
                start_pfn: 0x100,
                end_pfn: 0x100,
                free_pfn_count: 0,
                contiguous_required: 1,
            },
            stats: CompactionStats::default(),
        };
        assert_eq!(compact_zone(&mut zone), CompactionResult::Failed);
    }

    #[test]
    fn test_compact_zone_zero_order() {
        let _guard = crate::test_serial::acquire();
        let mut zone = CompactZone {
            zone: CompactionZone {
                name: "test",
                start_pfn: 0x100,
                end_pfn: 0x200,
                free_pfn_count: 128,
                contiguous_required: 0,
            },
            stats: CompactionStats::default(),
        };
        assert_eq!(compact_zone(&mut zone), CompactionResult::Failed);
    }

    #[test]
    fn test_compact_zone_stats_accumulate() {
        let _guard = crate::test_serial::acquire();
        let mut zone = CompactZone {
            zone: CompactionZone {
                name: "test",
                start_pfn: 0x100,
                end_pfn: 0x110,
                free_pfn_count: 8,
                contiguous_required: 1,
            },
            stats: CompactionStats::default(),
        };
        let _ = compact_zone(&mut zone);
        assert!(zone.stats.scanned > 0);
    }

    #[test]
    fn test_compact_order_returns_some() {
        let _guard = crate::test_serial::acquire();
        let result = compact_order(0);
        assert!(result.is_some());
    }

    #[test]
    fn test_defrag_zone() {
        let _guard = crate::test_serial::acquire();
        let mut zone = CompactZone {
            zone: CompactionZone {
                name: "test",
                start_pfn: 0x100,
                end_pfn: 0x200,
                free_pfn_count: 128,
                contiguous_required: 64,
            },
            stats: CompactionStats::default(),
        };
        defrag_zone(&mut zone);
        assert_eq!(zone.zone.contiguous_required, 1);
    }

    #[test]
    fn test_global_compaction_stats() {
        let _guard = crate::test_serial::acquire();
        reset_compaction_stats();
        let before = compaction_stats();
        assert_eq!(before.migrated, 0);
        assert_eq!(before.failed, 0);
        assert_eq!(before.scanned, 0);
        let _ = compact_order(0);
        let after = compaction_stats();
        assert!(after.scanned > 0 || after.migrated > 0 || after.failed > 0);
    }

    #[test]
    fn test_reset_compaction_stats() {
        let _guard = crate::test_serial::acquire();
        let _ = compact_order(1);
        reset_compaction_stats();
        let s = compaction_stats();
        assert_eq!(s.migrated, 0);
        assert_eq!(s.failed, 0);
        assert_eq!(s.scanned, 0);
    }
}
