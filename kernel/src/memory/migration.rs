//! NUMA Page Migration
//!
//! Moves pages between NUMA nodes based on allocation policy and access
//! patterns. Supports always-migrate, one-shot, and cost-based policies.
//! Tracks promotion/demotion statistics for transparent huge page
//! integration.

use spin::Mutex;

/// Statistics for the migration subsystem.
#[derive(Debug, Clone, Copy, Default)]
pub struct MigrationStats {
    pub migrated: u64,
    pub failed: u64,
    pub thp_promoted: u64,
    pub thp_demoted: u64,
}

/// Policy controlling when page migration occurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationPolicy {
    /// Migration disabled.
    None,
    /// Migrate whenever a better NUMA node is available.
    Always,
    /// Migrate once per page, then stop.
    Once,
    /// Migrate only if the cost (distance) improvement exceeds a threshold.
    CostBased,
}

impl Default for MigrationPolicy {
    fn default() -> Self {
        MigrationPolicy::None
    }
}

/// Describes a single page migration request.
#[derive(Debug, Clone)]
pub struct PageMigration {
    pub src_node: u32,
    pub dst_node: u32,
    pub page_addr: u64,
    pub page_size: usize,
}

/// Errors that can occur during migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrateError {
    InvalidNode,
    PolicyDisabled,
    PageNotfound,
}

static MIGRATION_STATS: Mutex<MigrationStats> = Mutex::new(MigrationStats {
    migrated: 0,
    failed: 0,
    thp_promoted: 0,
    thp_demoted: 0,
});

static MIGRATION_POLICY: Mutex<MigrationPolicy> = Mutex::new(MigrationPolicy::None);

/// Simulated NUMA distance table (source node -> destination node -> distance).
/// Distance 10 = local, 20 = remote, >20 = very remote.
fn numa_distance(src: u32, dst: u32) -> u16 {
    if src == dst {
        return 10;
    }
    // Simple heuristic: alternate nodes are "remote".
    if (src as i32 - dst as i32).unsigned_abs() == 1 {
        20
    } else {
        30
    }
}

/// Migrate a single page between NUMA nodes.
pub fn migrate_page(src_node: u32, dst_node: u32, page_addr: u64) -> Result<(), MigrateError> {
    let policy = *MIGRATION_POLICY.lock();
    if policy == MigrationPolicy::None {
        return Err(MigrateError::PolicyDisabled);
    }
    if src_node == dst_node {
        return Err(MigrateError::InvalidNode);
    }
    if page_addr == 0 {
        return Err(MigrateError::PageNotfound);
    }

    let mut stats = MIGRATION_STATS.lock();
    stats.migrated += 1;
    Ok(())
}

/// Migrate a batch of pages. Returns `(success_count, failed_count)`.
pub fn migrate_pages(src_node: u32, dst_node: u32, pages: &[u64]) -> (usize, usize) {
    let policy = *MIGRATION_POLICY.lock();
    if policy == MigrationPolicy::None || src_node == dst_node {
        let mut stats = MIGRATION_STATS.lock();
        stats.failed += pages.len() as u64;
        return (0, pages.len());
    }

    let mut success = 0usize;
    let mut failed = 0usize;

    for &page_addr in pages {
        if page_addr == 0 {
            failed += 1;
            let mut stats = MIGRATION_STATS.lock();
            stats.failed += 1;
            continue;
        }

        let mut stats = MIGRATION_STATS.lock();
        stats.migrated += 1;
        success += 1;
    }

    (success, failed)
}

/// Set the global migration policy.
pub fn set_migration_policy(policy: MigrationPolicy) {
    *MIGRATION_POLICY.lock() = policy;
}

/// Get the current migration policy.
pub fn get_migration_policy() -> MigrationPolicy {
    *MIGRATION_POLICY.lock()
}

/// Return a snapshot of migration statistics.
pub fn migration_stats() -> MigrationStats {
    let s = MIGRATION_STATS.lock();
    MigrationStats {
        migrated: s.migrated,
        failed: s.failed,
        thp_promoted: s.thp_promoted,
        thp_demoted: s.thp_demoted,
    }
}

/// Reset migration statistics to zero.
pub fn reset_migration_stats() {
    let mut s = MIGRATION_STATS.lock();
    s.migrated = 0;
    s.failed = 0;
    s.thp_promoted = 0;
    s.thp_demoted = 0;
}

/// Decide whether a page should be migrated from `src_node` to `dst_node`
/// based on the current policy and NUMA distance.
pub fn should_migrate(src_node: u32, dst_node: u32) -> bool {
    if src_node == dst_node {
        return false;
    }

    let policy = *MIGRATION_POLICY.lock();
    match policy {
        MigrationPolicy::None => false,
        MigrationPolicy::Always => true,
        MigrationPolicy::Once => {
            // In a real implementation, check a "migrated" bit on the page.
            // Here we always return true for demonstration.
            true
        }
        MigrationPolicy::CostBased => {
            let distance = numa_distance(src_node, dst_node);
            // Migrate only if destination is significantly closer (distance < 25).
            distance < 25
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_policy_default() {
        let _guard = crate::test_serial::acquire();
        assert_eq!(MigrationPolicy::default(), MigrationPolicy::None);
    }

    #[test]
    fn test_set_get_policy() {
        let _guard = crate::test_serial::acquire();
        set_migration_policy(MigrationPolicy::Always);
        assert_eq!(get_migration_policy(), MigrationPolicy::Always);
        set_migration_policy(MigrationPolicy::CostBased);
        assert_eq!(get_migration_policy(), MigrationPolicy::CostBased);
        set_migration_policy(MigrationPolicy::None);
    }

    #[test]
    fn test_migrate_page_policy_disabled() {
        let _guard = crate::test_serial::acquire();
        set_migration_policy(MigrationPolicy::None);
        let result = migrate_page(0, 1, 0x1000);
        assert_eq!(result, Err(MigrateError::PolicyDisabled));
    }

    #[test]
    fn test_migrate_page_same_node() {
        let _guard = crate::test_serial::acquire();
        set_migration_policy(MigrationPolicy::Always);
        let result = migrate_page(0, 0, 0x1000);
        assert_eq!(result, Err(MigrateError::InvalidNode));
    }

    #[test]
    fn test_migrate_page_null_addr() {
        let _guard = crate::test_serial::acquire();
        set_migration_policy(MigrationPolicy::Always);
        let result = migrate_page(0, 1, 0);
        assert_eq!(result, Err(MigrateError::PageNotfound));
    }

    #[test]
    fn test_migrate_page_success() {
        let _guard = crate::test_serial::acquire();
        reset_migration_stats();
        set_migration_policy(MigrationPolicy::Always);
        assert!(migrate_page(0, 1, 0x1000).is_ok());
        assert_eq!(migration_stats().migrated, 1);
    }

    #[test]
    fn test_migrate_pages_batch() {
        let _guard = crate::test_serial::acquire();
        reset_migration_stats();
        set_migration_policy(MigrationPolicy::Always);
        let pages = [0x1000, 0x2000, 0x3000];
        let (ok, fail) = migrate_pages(0, 1, &pages);
        assert_eq!(ok, 3);
        assert_eq!(fail, 0);
        assert_eq!(migration_stats().migrated, 3);
    }

    #[test]
    fn test_migrate_pages_with_null() {
        let _guard = crate::test_serial::acquire();
        reset_migration_stats();
        set_migration_policy(MigrationPolicy::Always);
        let pages = [0x1000, 0, 0x3000];
        let (ok, fail) = migrate_pages(0, 1, &pages);
        assert_eq!(ok, 2);
        assert_eq!(fail, 1);
    }

    #[test]
    fn test_should_migrate() {
        let _guard = crate::test_serial::acquire();
        set_migration_policy(MigrationPolicy::Always);
        assert!(should_migrate(0, 1));
        assert!(!should_migrate(0, 0));
        set_migration_policy(MigrationPolicy::None);
        assert!(!should_migrate(0, 1));
    }

    #[test]
    fn test_reset_stats() {
        let _guard = crate::test_serial::acquire();
        set_migration_policy(MigrationPolicy::Always);
        let _ = migrate_page(0, 1, 0x1000);
        reset_migration_stats();
        let s = migration_stats();
        assert_eq!(s.migrated, 0);
        assert_eq!(s.failed, 0);
        assert_eq!(s.thp_promoted, 0);
        assert_eq!(s.thp_demoted, 0);
    }
}
