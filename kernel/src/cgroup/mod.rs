use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

const MAX_CGROUPS: usize = 64;
const MAX_PIDS: usize = 1024;

/// Lock-free per-cgroup memory usage counters (bytes).
/// Indexed by cgroup slot index. Used by the heap allocator path.
static CGROUP_MEM_USED: [AtomicU64; MAX_CGROUPS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; MAX_CGROUPS]
};

/// Lock-free PID → cgroup index mapping.
/// -1 means the PID has no cgroup assignment.
static PID_TO_CGROUP_IDX: [AtomicI32; MAX_PIDS] = {
    const NONE: AtomicI32 = AtomicI32::new(-1);
    [NONE; MAX_PIDS]
};

/// Next cgroup slot index (monotonically increasing).
static CGROUP_NEXT_IDX: AtomicUsize = AtomicUsize::new(0);

/// Total heap bytes currently allocated across all cgroups (for diagnostics).
static HEAP_CGROUP_TOTAL: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum CgroupController {
    cpu,
    memory,
    pids,
    io,
}

pub struct Cgroup {
    pub name: String,
    pub path: String,
    pub parent_path: String,
    pub procs: Vec<u32>,
    pub cpu_max: Option<u64>,
    pub memory_max: Option<u64>,
    pub pids_max: Option<u32>,
    pub controllers: Vec<CgroupController>,
    /// CPU usage: accumulated ticks in current period.
    pub cpu_used: u64,
    /// CPU quota: max ticks per period (same value as cpu_max).
    pub cpu_period_ticks: u64,
    /// Memory usage: current RSS in bytes.
    pub memory_used: u64,
    /// Lock-free slot index into CGROUP_MEM_USED array.
    pub cgroup_idx: u32,
}

static CGROUPS: Mutex<Vec<Cgroup>> = Mutex::new(Vec::new());

/// Global CPU accounting: total ticks consumed across all cgroups this period.
static CPU_TOTAL_TICKS: AtomicU64 = AtomicU64::new(0);

pub const CPU_PERIOD_MS: u64 = 100; // 100ms period

pub fn init() {
    let mut groups = CGROUPS.lock();
    let root_idx = CGROUP_NEXT_IDX.fetch_add(1, Ordering::Relaxed) as u32;
    groups.push(Cgroup {
        name: String::from("/"),
        path: String::from("/"),
        parent_path: String::new(),
        procs: Vec::new(),
        cpu_max: None,
        memory_max: None,
        pids_max: None,
        controllers: alloc::vec![
            CgroupController::cpu,
            CgroupController::memory,
            CgroupController::pids,
            CgroupController::io,
        ],
        cpu_used: 0,
        cpu_period_ticks: CPU_PERIOD_MS,
        memory_used: 0,
        cgroup_idx: root_idx,
    });

    // Create well-known sub-cgroups for system services
    drop(groups);
    let _ = cgroup_create("/", "system");
    let _ = cgroup_create("/", "user");
    let _ = cgroup_create("/", "daemon");

    // Apply default limits to the daemon cgroup (prevents runaway processes)
    {
        let mut groups = CGROUPS.lock();
        if let Some(damon) = groups.iter_mut().find(|g| g.path == "/daemon") {
            damon.pids_max = Some(64);
            damon.memory_max = Some(256 * 1024 * 1024); // 256 MB
            damon.cpu_max = Some(CPU_PERIOD_MS * 80 / 100); // 80% of period
        }
        if let Some(system) = groups.iter_mut().find(|g| g.path == "/system") {
            system.pids_max = Some(256);
            system.memory_max = Some(512 * 1024 * 1024); // 512 MB
        }
    }

    crate::serial::println!(
        "[CGROUP] Initialized: root cgroup with cpu, memory, pids, io controllers"
    );
    crate::serial::println!(
        "[CGROUP] Created /system (256 procs, 512MB), /user, /daemon (64 procs, 256MB)"
    );
}

fn resolve_path(base: &str, name: &str) -> String {
    if base == "/" {
        alloc::format!("/{}", name)
    } else {
        alloc::format!("{}/{}", base, name)
    }
}

pub fn cgroup_create(parent_path: &str, name: &str) -> Result<(), i32> {
    let path = resolve_path(parent_path, name);
    let mut groups = CGROUPS.lock();
    if groups.iter().any(|g| g.path == path) {
        return Err(17); // EEXIST
    }
    let parent = groups.iter().find(|g| g.path == parent_path).ok_or(2)?;
    let cpu_max = parent.cpu_max;
    let memory_max = parent.memory_max;
    let pids_max = parent.pids_max;
    let controllers = parent.controllers.clone();
    let cgroup_idx = CGROUP_NEXT_IDX.fetch_add(1, Ordering::Relaxed) as u32;
    groups.push(Cgroup {
        name: String::from(name),
        path,
        parent_path: String::from(parent_path),
        procs: Vec::new(),
        cpu_max,
        memory_max,
        pids_max,
        controllers,
        cpu_used: 0,
        cpu_period_ticks: 0,
        memory_used: 0,
        cgroup_idx,
    });
    Ok(())
}

pub fn cgroup_add_process(path: &str, pid: u32) -> Result<(), i32> {
    let mut groups = CGROUPS.lock();
    let cgroup = groups.iter_mut().find(|g| g.path == path).ok_or(2)?;
    if cgroup
        .pids_max
        .is_some_and(|max| cgroup.procs.len() >= max as usize)
    {
        return Err(28); // ENOSPC
    }
    if !cgroup.procs.contains(&pid) {
        cgroup.procs.push(pid);
        // Update lock-free PID → cgroup index mapping
        let idx = pid as usize;
        if idx < MAX_PIDS {
            PID_TO_CGROUP_IDX[idx].store(cgroup.cgroup_idx as i32, Ordering::Relaxed);
        }
    }
    Ok(())
}

pub fn cgroup_set_cpu_max(path: &str, max: u64) -> Result<(), i32> {
    let mut groups = CGROUPS.lock();
    let cgroup = groups.iter_mut().find(|g| g.path == path).ok_or(2)?;
    cgroup.cpu_max = Some(max);
    cgroup.cpu_period_ticks = max;
    Ok(())
}

pub fn cgroup_set_memory_max(path: &str, max: u64) -> Result<(), i32> {
    let mut groups = CGROUPS.lock();
    let cgroup = groups.iter_mut().find(|g| g.path == path).ok_or(2)?;
    cgroup.memory_max = Some(max);
    Ok(())
}

pub fn cgroup_set_pids_max(path: &str, max: u32) -> Result<(), i32> {
    let mut groups = CGROUPS.lock();
    let cgroup = groups.iter_mut().find(|g| g.path == path).ok_or(2)?;
    cgroup.pids_max = Some(max);
    Ok(())
}

/// Record one tick of CPU usage for a process's cgroup.
/// Returns false if the cgroup has exceeded its cpu_max quota.
pub fn cgroup_cpu_tick(pid: u32) -> bool {
    let mut groups = CGROUPS.lock();
    if let Some(cgroup) = groups.iter_mut().find(|g| g.procs.contains(&pid)) {
        cgroup.cpu_used += 1;
        if let Some(max) = cgroup.cpu_max {
            return cgroup.cpu_used <= max;
        }
    }
    true
}

/// Reset all cgroup CPU accounting (called at period boundary).
pub fn cgroup_cpu_reset_period() {
    let mut groups = CGROUPS.lock();
    for g in groups.iter_mut() {
        g.cpu_used = 0;
    }
    CPU_TOTAL_TICKS.store(0, Ordering::Relaxed);
}

/// Record memory allocation for a process's cgroup.
/// Returns false (and does not modify state) if the cgroup would exceed its memory_max.
pub fn cgroup_memory_alloc(pid: u32, bytes: u64) -> bool {
    let mut groups = CGROUPS.lock();
    if let Some(cgroup) = groups.iter_mut().find(|g| g.procs.contains(&pid)) {
        if let Some(max) = cgroup.memory_max
            && cgroup.memory_used + bytes > max
        {
            return false;
        }
        cgroup.memory_used += bytes;
        // Update lock-free atomic counter
        let idx = cgroup.cgroup_idx as usize;
        if idx < MAX_CGROUPS {
            CGROUP_MEM_USED[idx].fetch_add(bytes, Ordering::Relaxed);
        }
    }
    true
}

/// Record memory deallocation for a process's cgroup.
pub fn cgroup_memory_free(pid: u32, bytes: u64) {
    let mut groups = CGROUPS.lock();
    if let Some(cgroup) = groups.iter_mut().find(|g| g.procs.contains(&pid)) {
        cgroup.memory_used = cgroup.memory_used.saturating_sub(bytes);
        // Update lock-free atomic counter
        let idx = cgroup.cgroup_idx as usize;
        if idx < MAX_CGROUPS {
            CGROUP_MEM_USED[idx].fetch_sub(bytes.min(CGROUP_MEM_USED[idx].load(Ordering::Relaxed)), Ordering::Relaxed);
        }
    }
}

// ---------------------------------------------------------------------------
// Lock-free cgroup memory accounting (safe for use from heap allocator)
// ---------------------------------------------------------------------------

/// Lock-free check-and-reserve: returns true if the allocation is within the
/// cgroup's memory limit and the reservation succeeded. Returns false if the
/// allocation would exceed the limit (caller must NOT allocate).
///
/// This function never acquires a mutex and is safe to call from within the
/// heap allocator's `GlobalAlloc` implementation.
pub fn cgroup_try_reserve_memory(pid: u32, bytes: u64) -> bool {
    let pidx = pid as usize;
    if pidx >= MAX_PIDS {
        return true; // Unknown PID — allow (no cgroup limit)
    }
    let cgroup_idx = PID_TO_CGROUP_IDX[pidx].load(Ordering::Relaxed);
    if cgroup_idx < 0 {
        return true; // No cgroup assigned — allow
    }
    let idx = cgroup_idx as usize;
    if idx >= MAX_CGROUPS {
        return true;
    }

    // We need the memory_max from the CGROUPS data. To avoid taking the
    // mutex, we store memory_max in an atomic too. For now, use a try_lock
    // on the CGROUPS mutex — if contended, allow the allocation (best-effort).
    if let Some(groups) = CGROUPS.try_lock() {
        if let Some(cgroup) = groups.iter().find(|g| g.cgroup_idx == cgroup_idx as u32) {
            if let Some(max) = cgroup.memory_max {
                let current = CGROUP_MEM_USED[idx].load(Ordering::Relaxed);
                if current + bytes > max {
                    return false;
                }
            }
        }
    }
    // Always reserve on success path so the caller can allocate
    CGROUP_MEM_USED[idx].fetch_add(bytes, Ordering::Relaxed);
    HEAP_CGROUP_TOTAL.fetch_add(bytes, Ordering::Relaxed);
    true
}

/// Undo a reservation made by `cgroup_try_reserve_memory`.
/// Called when an allocation fails after reservation, or on deallocation.
pub fn cgroup_release_memory(pid: u32, bytes: u64) {
    let pidx = pid as usize;
    if pidx >= MAX_PIDS {
        return;
    }
    let cgroup_idx = PID_TO_CGROUP_IDX[pidx].load(Ordering::Relaxed);
    if cgroup_idx < 0 {
        return;
    }
    let idx = cgroup_idx as usize;
    if idx >= MAX_CGROUPS {
        return;
    }
    let current = CGROUP_MEM_USED[idx].load(Ordering::Relaxed);
    CGROUP_MEM_USED[idx].fetch_sub(bytes.min(current), Ordering::Relaxed);
    let total = HEAP_CGROUP_TOTAL.load(Ordering::Relaxed);
    HEAP_CGROUP_TOTAL.fetch_sub(bytes.min(total), Ordering::Relaxed);
}

/// Total heap bytes tracked across all cgroups (diagnostic).
pub fn cgroup_heap_tracked_bytes() -> u64 {
    HEAP_CGROUP_TOTAL.load(Ordering::Relaxed)
}

/// Check if a process's cgroup has exceeded its memory limit.
pub fn cgroup_memory_exceeded(pid: u32) -> bool {
    let groups = CGROUPS.lock();
    if let Some(cgroup) = groups.iter().find(|g| g.procs.contains(&pid))
        && let Some(max) = cgroup.memory_max
    {
        return cgroup.memory_used > max;
    }
    false
}

/// Find the cgroup path for a given PID.
pub fn cgroup_find_for_pid(pid: u32) -> Option<String> {
    let groups = CGROUPS.lock();
    groups
        .iter()
        .find(|g| g.procs.contains(&pid))
        .map(|g| g.path.clone())
}

#[cfg(test)]
pub fn reset_for_test() {
    let mut groups = CGROUPS.lock();
    groups.clear();
    groups.push(Cgroup {
        name: String::from("/"),
        path: String::from("/"),
        parent_path: String::new(),
        procs: Vec::new(),
        cpu_max: None,
        memory_max: None,
        pids_max: None,
        controllers: Vec::new(),
        cpu_used: 0,
        cpu_period_ticks: 0,
        memory_used: 0,
        cgroup_idx: 0,
    });
    // Reset lock-free atomic arrays
    CGROUP_NEXT_IDX.store(1, Ordering::Relaxed);
    for entry in &PID_TO_CGROUP_IDX {
        entry.store(-1, Ordering::Relaxed);
    }
    for entry in &CGROUP_MEM_USED {
        entry.store(0, Ordering::Relaxed);
    }
    HEAP_CGROUP_TOTAL.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() {
        reset_for_test();
    }

    #[test]
    fn create_cgroup_under_root() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert!(cgroup_create("/", "test_group").is_ok());
        let groups = CGROUPS.lock();
        assert!(groups.iter().any(|g| g.path == "/test_group"));
    }

    #[test]
    fn create_duplicate_cgroup_fails() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert!(cgroup_create("/", "dup").is_ok());
        assert_eq!(cgroup_create("/", "dup"), Err(17)); // EEXIST
    }

    #[test]
    fn create_cgroup_nonexistent_parent_fails() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert_eq!(cgroup_create("/nonexistent", "child"), Err(2)); // ENOENT
    }

    #[test]
    fn add_process_to_cgroup() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "grp").unwrap();
        assert!(cgroup_add_process("/grp", 100).is_ok());
        assert_eq!(cgroup_find_for_pid(100), Some(String::from("/grp")));
    }

    #[test]
    fn add_process_to_nonexistent_cgroup_fails() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert_eq!(cgroup_add_process("/nope", 1), Err(2)); // ENOENT
    }

    #[test]
    fn pid_max_enforced() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "limited").unwrap();
        cgroup_set_pids_max("/limited", 2).unwrap();
        assert!(cgroup_add_process("/limited", 1).is_ok());
        assert!(cgroup_add_process("/limited", 2).is_ok());
        assert_eq!(cgroup_add_process("/limited", 3), Err(28)); // ENOSPC
    }

    #[test]
    fn cpu_tick_accounting() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "cpu_grp").unwrap();
        cgroup_set_cpu_max("/cpu_grp", 3).unwrap();
        cgroup_add_process("/cpu_grp", 10).unwrap();

        assert!(cgroup_cpu_tick(10));
        assert!(cgroup_cpu_tick(10));
        assert!(cgroup_cpu_tick(10));
        // Fourth tick exceeds quota
        assert!(!cgroup_cpu_tick(10));
    }

    #[test]
    fn cpu_period_reset() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "cpu_reset").unwrap();
        cgroup_set_cpu_max("/cpu_reset", 2).unwrap();
        cgroup_add_process("/cpu_reset", 20).unwrap();

        cgroup_cpu_tick(20);
        cgroup_cpu_tick(20);
        assert!(!cgroup_cpu_tick(20));

        cgroup_cpu_reset_period();
        assert!(cgroup_cpu_tick(20));
    }

    #[test]
    fn memory_accounting() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "mem_grp").unwrap();
        cgroup_set_memory_max("/mem_grp", 1000).unwrap();
        cgroup_add_process("/mem_grp", 30).unwrap();

        assert!(cgroup_memory_alloc(30, 500));
        assert!(!cgroup_memory_exceeded(30));

        // 500 + 600 = 1100 > 1000 — allocation is rejected (returns false)
        assert!(!cgroup_memory_alloc(30, 600));
        // Usage should still be 500 since the 600-byte alloc was rejected
        assert!(!cgroup_memory_exceeded(30));
    }

    #[test]
    fn memory_free_reduces_usage() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "mem_free").unwrap();
        cgroup_set_memory_max("/mem_free", 1000).unwrap();
        cgroup_add_process("/mem_free", 40).unwrap();

        cgroup_memory_alloc(40, 800);
        cgroup_memory_free(40, 300);

        let groups = CGROUPS.lock();
        let cg = groups.iter().find(|g| g.path == "/mem_free").unwrap();
        assert_eq!(cg.memory_used, 500);
    }

    #[test]
    fn cpu_tick_unknown_pid_succeeds() {
        let _guard = crate::test_serial::acquire();
        setup();
        // Unknown PID should return true (no cgroup limit applies)
        assert!(cgroup_cpu_tick(9999));
    }

    #[test]
    fn memory_alloc_unknown_pid_succeeds() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert!(cgroup_memory_alloc(9999, 100));
    }

    #[test]
    fn nested_cgroup_inherits_limits() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "parent").unwrap();
        cgroup_set_cpu_max("/parent", 10).unwrap();
        cgroup_create("/parent", "child").unwrap();

        let groups = CGROUPS.lock();
        let child = groups.iter().find(|g| g.path == "/parent/child").unwrap();
        assert_eq!(child.cpu_max, Some(10));
    }

    // -----------------------------------------------------------------------
    // Boundary and edge-case tests
    // -----------------------------------------------------------------------

    #[test]
    fn memory_exact_at_limit_succeeds() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "exact").unwrap();
        cgroup_set_memory_max("/exact", 1000).unwrap();
        cgroup_add_process("/exact", 50).unwrap();
        // Allocate exactly to the limit
        assert!(cgroup_memory_alloc(50, 1000));
        let groups = CGROUPS.lock();
        let cg = groups.iter().find(|g| g.path == "/exact").unwrap();
        assert_eq!(cg.memory_used, 1000);
    }

    #[test]
    fn memory_one_byte_over_limit_rejected() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "over").unwrap();
        cgroup_set_memory_max("/over", 1000).unwrap();
        cgroup_add_process("/over", 51).unwrap();
        assert!(cgroup_memory_alloc(51, 999));
        assert!(!cgroup_memory_alloc(51, 2), "should reject 1 byte over limit");
    }

    #[test]
    fn memory_free_beyond_usage_saturates_to_zero() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "sat").unwrap();
        cgroup_set_memory_max("/sat", 1000).unwrap();
        cgroup_add_process("/sat", 52).unwrap();
        cgroup_memory_alloc(52, 100);
        cgroup_memory_free(52, 500); // free more than used
        let groups = CGROUPS.lock();
        let cg = groups.iter().find(|g| g.path == "/sat").unwrap();
        assert_eq!(cg.memory_used, 0, "saturating_sub should produce 0");
    }

    #[test]
    fn memory_exceeded_at_limit_boundary() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "boundary").unwrap();
        cgroup_set_memory_max("/boundary", 500).unwrap();
        cgroup_add_process("/boundary", 53).unwrap();
        cgroup_memory_alloc(53, 500);
        // memory_used == memory_max: NOT exceeded (strictly greater)
        assert!(!cgroup_memory_exceeded(53));
        // Free some then allocate to go over limit
        cgroup_memory_free(53, 100);
        // memory_used = 400; 400+200=600 > 500, should be rejected
        assert!(!cgroup_memory_alloc(53, 200), "400+200=600 > 500 should be rejected");
        // Free all, then alloc exactly to limit
        cgroup_memory_free(53, 500); // saturates to 0
        assert!(cgroup_memory_alloc(53, 500), "alloc exactly to limit should succeed");
        assert!(!cgroup_memory_exceeded(53), "exactly at limit is not exceeded");
    }

    #[test]
    fn multi_process_shares_cpu_quota() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "cpu_share").unwrap();
        cgroup_set_cpu_max("/cpu_share", 3).unwrap();
        cgroup_add_process("/cpu_share", 100).unwrap();
        cgroup_add_process("/cpu_share", 101).unwrap();
        // Both PIDs share the same quota pool of 3 ticks
        assert!(cgroup_cpu_tick(100));
        assert!(cgroup_cpu_tick(101));
        assert!(cgroup_cpu_tick(100));
        assert!(!cgroup_cpu_tick(101), "4th tick across both PIDs should exceed quota");
    }

    #[test]
    fn set_cpu_max_nonexistent_cgroup_fails() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert_eq!(cgroup_set_cpu_max("/nope", 10), Err(2));
    }

    #[test]
    fn set_memory_max_nonexistent_cgroup_fails() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert_eq!(cgroup_set_memory_max("/nope", 100), Err(2));
    }

    #[test]
    fn set_pids_max_nonexistent_cgroup_fails() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert_eq!(cgroup_set_pids_max("/nope", 10), Err(2));
    }

    #[test]
    fn init_creates_system_user_daemon() {
        let _guard = crate::test_serial::acquire();
        setup();
        init();
        let groups = CGROUPS.lock();
        assert!(groups.iter().any(|g| g.path == "/system"), "/system should exist");
        assert!(groups.iter().any(|g| g.path == "/user"), "/user should exist");
        assert!(groups.iter().any(|g| g.path == "/daemon"), "/daemon should exist");
        let daemon = groups.iter().find(|g| g.path == "/daemon").unwrap();
        assert_eq!(daemon.pids_max, Some(64));
        assert_eq!(daemon.memory_max, Some(256 * 1024 * 1024));
        let system = groups.iter().find(|g| g.path == "/system").unwrap();
        assert_eq!(system.pids_max, Some(256));
        assert_eq!(system.memory_max, Some(512 * 1024 * 1024));
    }

    #[test]
    fn deep_nested_cgroup_path() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "a").unwrap();
        cgroup_create("/a", "b").unwrap();
        cgroup_create("/a/b", "c").unwrap();
        let groups = CGROUPS.lock();
        assert!(groups.iter().any(|g| g.path == "/a/b/c"));
        let deep = groups.iter().find(|g| g.path == "/a/b/c").unwrap();
        assert_eq!(deep.parent_path, "/a/b");
    }

    #[test]
    fn add_duplicate_process_no_double_count() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "dedup").unwrap();
        cgroup_add_process("/dedup", 200).unwrap();
        cgroup_add_process("/dedup", 200).unwrap(); // duplicate
        let groups = CGROUPS.lock();
        let cg = groups.iter().find(|g| g.path == "/dedup").unwrap();
        assert_eq!(cg.procs.len(), 1, "duplicate PID should not be added twice");
    }

    #[test]
    fn remove_process_not_supported_yet() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "rmtest").unwrap();
        cgroup_add_process("/rmtest", 300).unwrap();
        // No explicit remove API; just verify the PID is there
        assert_eq!(cgroup_find_for_pid(300), Some(String::from("/rmtest")));
    }

    #[test]
    fn memory_alloc_multiple_processes_independent() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "multi").unwrap();
        cgroup_set_memory_max("/multi", 1000).unwrap();
        cgroup_add_process("/multi", 400).unwrap();
        cgroup_add_process("/multi", 401).unwrap();
        // Each PID contributes to the same memory_used
        assert!(cgroup_memory_alloc(400, 400));
        assert!(cgroup_memory_alloc(401, 400));
        assert!(!cgroup_memory_alloc(400, 201), "801+201=1002 > 1000 should fail");
        assert!(cgroup_memory_alloc(401, 199), "800+199=999 <= 1000 should succeed");
    }

    // -----------------------------------------------------------------------
    // Lock-free cgroup memory accounting tests
    // -----------------------------------------------------------------------

    #[test]
    fn lockfree_try_reserve_within_limit() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "lf_ok").unwrap();
        cgroup_set_memory_max("/lf_ok", 1000).unwrap();
        cgroup_add_process("/lf_ok", 600).unwrap();
        assert!(cgroup_try_reserve_memory(600, 500));
        assert!(cgroup_try_reserve_memory(600, 400));
        // 500 + 400 = 900, next 200 would exceed
        assert!(!cgroup_try_reserve_memory(600, 200));
    }

    #[test]
    fn lockfree_try_reserve_unknown_pid_allows() {
        let _guard = crate::test_serial::acquire();
        setup();
        // Unknown PID should always be allowed (no cgroup limit)
        assert!(cgroup_try_reserve_memory(9999, 1_000_000));
    }

    #[test]
    fn lockfree_release_reduces_counter() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "lf_rel").unwrap();
        cgroup_set_memory_max("/lf_rel", 2000).unwrap();
        cgroup_add_process("/lf_rel", 700).unwrap();
        assert!(cgroup_try_reserve_memory(700, 1000));
        cgroup_release_memory(700, 500);
        // After releasing 500, should be able to reserve another 1500
        assert!(cgroup_try_reserve_memory(700, 1500));
    }

    #[test]
    fn lockfree_heap_tracked_bytes() {
        let _guard = crate::test_serial::acquire();
        setup();
        let before = cgroup_heap_tracked_bytes();
        cgroup_create("/", "lf_track").unwrap();
        cgroup_set_memory_max("/lf_track", 5000).unwrap();
        cgroup_add_process("/lf_track", 800).unwrap();
        assert!(cgroup_try_reserve_memory(800, 100));
        assert!(cgroup_heap_tracked_bytes() >= before + 100);
        cgroup_release_memory(800, 100);
        assert!(cgroup_heap_tracked_bytes() <= before + 1);
    }

    #[test]
    fn lockfree_pid_to_cgroup_mapping() {
        let _guard = crate::test_serial::acquire();
        setup();
        cgroup_create("/", "lf_map").unwrap();
        cgroup_add_process("/lf_map", 555).unwrap();
        // PID 555 should map to the lf_map cgroup's slot
        let idx = PID_TO_CGROUP_IDX[555].load(Ordering::Relaxed);
        assert!(idx >= 0, "PID 555 should have a cgroup index");
        // Verify the atomic counter is accessible
        let mem = CGROUP_MEM_USED[idx as usize].load(Ordering::Relaxed);
        assert_eq!(mem, 0, "fresh cgroup should have 0 memory");
    }
}
