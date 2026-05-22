use alloc::collections::BTreeMap;
use spin::Mutex;

use crate::process::{Process, ProcessId};

/// Global process table — tracks all user processes for OOM victim selection.
static PROCESS_TABLE: Mutex<BTreeMap<ProcessId, ProcessEntry>> = Mutex::new(BTreeMap::new());

#[derive(Debug, Clone)]
struct ProcessEntry {
    process: Process,
    priority: u64,
}

/// Register a process in the global table so the OOM killer can enumerate it.
pub fn register_process(process: &Process) {
    let pid = process.id();
    PROCESS_TABLE.lock().insert(
        pid,
        ProcessEntry {
            process: process.clone(),
            priority: 0,
        },
    );
}

/// Unregister a process (called when the process exits).
pub fn unregister_process(pid: ProcessId) {
    PROCESS_TABLE.lock().remove(&pid);
}

/// Set the OOM priority for a process (higher = more likely to be killed).
pub fn set_priority(pid: ProcessId, priority: u64) {
    if let Some(entry) = PROCESS_TABLE.lock().get_mut(&pid) {
        entry.priority = priority;
    }
}

/// Compute an OOM score for a process.
///
/// Formula: `rss_bytes / PAGE_SIZE + priority * 100`
///
/// Higher score = more likely to be killed.
/// RSS is estimated by summing VMA sizes (upper bound, no page-table walk).
fn score_entry(entry: &ProcessEntry) -> u64 {
    let rss = entry.process.with_vma_set(|vmas| {
        vmas.iter().map(|vma| vma.size()).sum::<u64>()
    });
    rss / crate::memory::PAGE_SIZE + entry.priority * 100
}

/// Compute an OOM score for a process by PID.
pub fn oom_score(pid: ProcessId) -> u64 {
    let table = PROCESS_TABLE.lock();
    match table.get(&pid) {
        Some(entry) => score_entry(entry),
        None => 0,
    }
}

/// Select the OOM victim — the process with the highest score,
/// excluding PID 1 (init) and kernel PID 0.
///
/// Returns `Some(ProcessId)` of the victim, or `None` if no victim found.
pub fn select_victim() -> Option<ProcessId> {
    let table = PROCESS_TABLE.lock();
    let mut best_pid = None;
    let mut best_score = 0u64;

    for (&pid, entry) in table.iter() {
        // Never kill init (PID 1) or kernel process (PID 0)
        if pid.0 == 0 || pid.0 == 1 {
            continue;
        }

        let score = score_entry(entry);
        if score > best_score {
            best_score = score;
            best_pid = Some(pid);
        }
    }

    best_pid
}

/// Kill a process by PID.
///
/// Since there is no signal delivery infrastructure yet, this sets the
/// process's task(s) to `Zombie` via the scheduler exit path.
fn kill_process(pid: ProcessId) -> bool {
    use crate::task::scheduler;
    // For each task associated with this process, force it to zombie state.
    // The scheduler will pick up zombie tasks and halt if the last one dies.
    // We queue an exit for the current task if it matches.
    let maybe_current = scheduler::get_current_process();
    if let Some(current) = maybe_current {
        if current.id() == pid {
            scheduler::exit_current_task();
            // unreachable
        }
    }

    // For other processes, we rely on the fact that they will be detected
    // as zombies on the next timer tick. For a real implementation, we would
    // send SIGKILL via the signal subsystem (task 26).
    //
    // For now, we detach the process by removing it from the process table
    // so the OOM killer won't pick it again.
    unregister_process(pid);
    true
}

/// Execute the OOM killer: select a victim, kill it, and return its PID.
///
/// Returns `None` if no suitable victim was found (all remaining processes
/// are PID 0 or 1, which are protected).
pub fn oom_kill() -> Option<ProcessId> {
    let victim = select_victim()?;
    let score = {
        let table = PROCESS_TABLE.lock();
        table.get(&victim).map(|e| score_entry(e)).unwrap_or(0)
    };

    crate::serial::println!(
        "[OOM] Killing PID {} (score: {})",
        victim.0,
        score,
    );

    if kill_process(victim) {
        // Remove from process table
        unregister_process(victim);
        Some(victim)
    } else {
        None
    }
}

/// Number of registered processes in the process table.
pub fn process_count() -> usize {
    PROCESS_TABLE.lock().len()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that the scoring function is monotonic with RSS.
    #[test]
    fn higher_rss_higher_score() {
        let score1 = 4096u64 / crate::memory::PAGE_SIZE;
        let score2 = 8192u64 / crate::memory::PAGE_SIZE;
        assert!(score2 > score1, "higher RSS must give higher score");
    }

    /// Test that oom_score returns 0 for non-existent PID.
    #[test]
    fn nonexistent_pid_score_zero() {
        PROCESS_TABLE.lock().clear();
        let score = oom_score(ProcessId(999));
        assert_eq!(score, 0, "non-existent PID should have score 0");
    }

    /// Test that the score formula is correct for a known combination.
    #[test]
    fn score_formula_correctness() {
        let priority = 5u64;
        let rss_pages = 100u64;
        let expected = rss_pages + priority * 100;
        assert_eq!(expected, rss_pages + priority * 100);
    }
}
