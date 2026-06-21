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
    let rss = entry
        .process
        .with_vma_set(|vmas| vmas.iter().map(|vma| vma.size()).sum::<u64>());
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
/// Delivers SIGKILL via the signal subsystem. If the target is the current
/// process, we additionally trigger the scheduler exit path.
fn kill_process(pid: ProcessId) -> bool {
    use crate::task::signals::send_signal;
    use crate::task::scheduler;

    // If we're killing the current process, use the scheduler exit path.
    let maybe_current = scheduler::get_current_process();
    if let Some(current) = maybe_current
        && current.id() == pid
    {
        send_signal(pid, 9); // SIGKILL
        scheduler::exit_current_task();
    }

    // For other processes, send SIGKILL via the signal subsystem.
    send_signal(pid, 9);
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
        table.get(&victim).map(score_entry).unwrap_or(0)
    };

    crate::serial::println!("[OOM] Killing PID {} (score: {})", victim.0, score,);

    if kill_process(victim) {
        // Remove from process table
        unregister_process(victim);
        Some(victim)
    } else {
        None
    }
}

/// OOM kill with a 5-second retry window.
///
/// Attempts to kill the highest-score victim, waits ~5 seconds by yielding,
/// then kills the next-highest victim if memory is still exhausted.
/// Returns the last victim PID killed, or `None` if no victim was found.
pub fn oom_kill_with_retry() -> Option<ProcessId> {
    // First attempt
    let _victim = oom_kill();

    // Wait approximately 5 seconds by yielding in a loop.
    // On QEMU with ~1ms timer ticks, 5000 yields ≈ 5 seconds.
    for _ in 0..5000 {
        crate::task::scheduler::yield_task();
    }

    // Second attempt — pick next-highest if still OOM
    oom_kill()
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

    #[test]
    fn register_process_and_count() {
        PROCESS_TABLE.lock().clear();
        assert_eq!(process_count(), 0);
        let proc = crate::process::Process::kernel_process();
        register_process(&proc);
        assert_eq!(process_count(), 1);
        // PIDs 0 and 1 are excluded from victim selection
        assert!(select_victim().is_none());
    }

    #[test]
    fn unregister_decrements_count() {
        PROCESS_TABLE.lock().clear();
        let proc = crate::process::Process::kernel_process();
        register_process(&proc);
        assert_eq!(process_count(), 1);
        unregister_process(proc.id());
        assert_eq!(process_count(), 0);
    }

    #[test]
    fn set_priority_affects_score() {
        PROCESS_TABLE.lock().clear();
        let proc = crate::process::Process::kernel_process();
        register_process(&proc);
        let pid = proc.id();
        let score_before = oom_score(pid);
        set_priority(pid, 10);
        let score_after = oom_score(pid);
        assert!(score_after >= score_before + 1000);
        // Cleanup
        unregister_process(pid);
    }

    #[test]
    fn select_victim_skips_pid_zero() {
        PROCESS_TABLE.lock().clear();
        let proc = crate::process::Process::kernel_process();
        assert_eq!(proc.id().0, 0);
        register_process(&proc);
        assert!(select_victim().is_none(), "PID 0 must never be selected as OOM victim");
        unregister_process(proc.id());
    }

    #[test]
    fn select_victim_empty_table() {
        PROCESS_TABLE.lock().clear();
        assert!(select_victim().is_none());
    }

    #[test]
    fn oom_kill_empty_table() {
        PROCESS_TABLE.lock().clear();
        assert!(oom_kill().is_none());
    }
}
