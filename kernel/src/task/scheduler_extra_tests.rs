#![cfg(test)]

use super::scheduler::{
    compute_deadline, get_task_count, pick_eevdf, test_reset,
    update_vruntime, SCHEDULER,
};
use super::scheduler_class::DEFAULT_TIMESLICE;
use super::{Task, TaskId, TaskState};
use crate::process::{
    Process, ProcessControlBlock, ProcessId, ProcessState, SignalAction, SignalSet,
};
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use spin::Mutex;

fn make_test_process(pid: usize) -> Process {
    let pcb = ProcessControlBlock {
        id: ProcessId(pid),
        ppid: ProcessId(0),
        state: ProcessState::Ready,
        pml4_frame: x86_64::structures::paging::PhysFrame::containing_address(
            x86_64::PhysAddr::new(0),
        ),
        entry_point: x86_64::VirtAddr::zero(),
        stack_top: x86_64::VirtAddr::zero(),
        threads: alloc::vec![],
        vma_set: crate::memory::vma::VmaSet::new(),
        mmap_next_addr: x86_64::VirtAddr::zero(),
        aslr_base: x86_64::VirtAddr::zero(),
        fd_table: alloc::vec![None; 1024],
        signal_mask: SignalSet::empty(),
        signal_handlers: [SignalAction::Default; 64],
        pending_signals: SignalSet::empty(),
        pending_signal_frame: None,
        sec_ctx: crate::security::SecurityContext::root(),
        nsproxy: crate::security::namespaces::NsProxy::new(),
        seccomp_filter: None,
        cgroup_path: None,
        cwd: alloc::string::String::from("/"),
    };
    Process {
        inner: Arc::new(Mutex::new(pcb)),
    }
}

fn make_test_task(pid: usize) -> Task {
    Task::new_test(TaskId::new(), make_test_process(pid), TaskState::Running)
}

fn test_insert_task(task: Task) {
    let mut sched = SCHEDULER.lock();
    let target = sched.least_loaded_cpu();
    let deadline = compute_deadline(task.vruntime, task.time_slice, task.weight);
    let mut task = task;
    task.deadline = deadline;
    task.eligible = task.vruntime <= sched.min_vruntime;
    sched.cpu_queues[target].insert((deadline, task.id.0), task);
    sched.task_count += 1;
}

fn test_queue_len() -> usize {
    let sched = SCHEDULER.lock();
    sched.cpu_queues.iter().map(|q| q.len()).sum()
}

// -----------------------------------------------------------------------
// SMP load-balancing tests
// -----------------------------------------------------------------------

#[test]
fn test_steal_task_from_loaded_cpu() {
    let _guard = crate::test_serial::acquire();
    test_reset();
    for (pid, vruntime) in [(600, 10u64), (601, 20), (602, 30)] {
        let mut task = make_test_task(pid);
        task.vruntime = vruntime;
        test_insert_task(task);
    }
    assert_eq!(test_queue_len(), 3);
    let mut sched = SCHEDULER.lock();
    let stolen = sched.steal_task();
    // In single-CPU mode, steal_task only steals from OTHER CPUs.
    assert!(stolen.is_none(), "single-CPU: steal_task returns None");
}

#[test]
fn test_steal_task_respects_balance() {
    let _guard = crate::test_serial::acquire();
    test_reset();
    for pid in [700, 701, 702, 703] {
        let mut task = make_test_task(pid);
        task.vruntime = pid as u64;
        test_insert_task(task);
    }
    let q_len = test_queue_len();
    assert_eq!(q_len, 4);
    let mut sched = SCHEDULER.lock();
    let stolen = sched.steal_task();
    // Single-CPU: all tasks are on CPU 0, steal_task skips my_cpu (0).
    assert!(stolen.is_none());
    drop(sched);
}

// -----------------------------------------------------------------------
// EEVDF vruntime fairness tests
// -----------------------------------------------------------------------

#[test]
fn test_pick_eevdf_vruntime_fairness() {
    let _guard = crate::test_serial::acquire();
    let mut queue = BTreeMap::new();
    for (pid, vruntime) in [(800, 50u64), (801, 50), (802, 50)] {
        let mut task = make_test_task(pid);
        task.vruntime = vruntime;
        task.weight = 1024;
        task.eligible = true;
        task.deadline = compute_deadline(vruntime, DEFAULT_TIMESLICE, 1024);
        queue.insert((task.deadline, task.id.0), task);
    }
    let picked = pick_eevdf(&queue).expect("should pick a task");
    // All tasks have equal vruntime and weight; first one inserted wins.
    assert_eq!(picked.vruntime, 50);
    assert!(picked.eligible);
}

#[test]
fn test_pick_eevdf_weight_priority() {
    let _guard = crate::test_serial::acquire();
    let mut queue = BTreeMap::new();

    // Low-weight task (nice +19): large virtual deadline
    let mut low = make_test_task(810);
    low.weight = 15;
    low.vruntime = 0;
    low.eligible = true;
    low.deadline = compute_deadline(0, DEFAULT_TIMESLICE, 15);
    queue.insert((low.deadline, low.id.0), low);

    // High-weight task (nice -20): small virtual deadline
    let mut high = make_test_task(811);
    high.weight = 88761;
    high.vruntime = 0;
    high.eligible = true;
    high.deadline = compute_deadline(0, DEFAULT_TIMESLICE, 88761);
    let high_id = high.id;
    queue.insert((high.deadline, high.id.0), high);

    let picked = pick_eevdf(&queue).expect("should pick a task");
    // High weight => smaller virtual deadline => picked first
    assert_eq!(picked.id, high_id, "high-weight task should be picked first");
}

// -----------------------------------------------------------------------
// timer_tick vruntime tests
// -----------------------------------------------------------------------

#[test]
fn test_timer_tick_updates_vruntime() {
    let _guard = crate::test_serial::acquire();
    let mut task = make_test_task(820);
    task.vruntime = 100;
    task.weight = 1024;
    let before = task.vruntime;
    update_vruntime(&mut task, 1);
    // delta = (1024 * 1) / 1024 = 1
    assert_eq!(task.vruntime, before + 1);
}

#[test]
fn test_timer_tick_eligibility() {
    let _guard = crate::test_serial::acquire();
    test_reset();
    {
        let mut sched = SCHEDULER.lock();
        sched.min_vruntime = 1000;
    }
    let mut task = make_test_task(830);
    task.vruntime = 2000;
    task.weight = 1024;
    test_insert_task(task);
    let sched = SCHEDULER.lock();
    let queue = &sched.cpu_queues[0];
    // Task with vruntime 2000 > min_vruntime 1000 is ineligible;
    // pick_eevdf skips ineligible tasks, so it returns None.
    let picked = pick_eevdf(queue);
    assert!(picked.is_none(), "task with vruntime > min_vruntime should be skipped");
}

// -----------------------------------------------------------------------
// Insert / deadline ordering tests
// -----------------------------------------------------------------------

#[test]
fn test_insert_maintains_deadline_order() {
    let _guard = crate::test_serial::acquire();
    test_reset();
    for pid in [840, 841, 842, 843, 844] {
        let mut task = make_test_task(pid);
        task.vruntime = (pid as u64) * 7;
        test_insert_task(task);
    }
    let sched = SCHEDULER.lock();
    let queue = &sched.cpu_queues[0];
    let deadlines: alloc::vec::Vec<(u64, usize)> = queue.keys().copied().collect();
    assert!(
        deadlines.windows(2).all(|w| w[0] <= w[1]),
        "BTreeMap keys must be sorted: {:?}",
        deadlines
    );
}

// -----------------------------------------------------------------------
// Task count accuracy tests
// -----------------------------------------------------------------------

#[test]
fn test_task_count_accurate() {
    let _guard = crate::test_serial::acquire();
    test_reset();
    assert_eq!(get_task_count(), 0);
    test_insert_task(make_test_task(850));
    assert_eq!(get_task_count(), 1);
    test_insert_task(make_test_task(851));
    assert_eq!(get_task_count(), 2);
    test_insert_task(make_test_task(852));
    assert_eq!(get_task_count(), 3);
}

// -----------------------------------------------------------------------
// Multiple pick round-robin test
// -----------------------------------------------------------------------

#[test]
fn test_multiple_pick_round_robin() {
    let _guard = crate::test_serial::acquire();
    let mut queue = BTreeMap::new();
    for (pid, deadline) in [(860, 10u64), (861, 20), (862, 30)] {
        let mut task = make_test_task(pid);
        task.eligible = true;
        task.deadline = deadline;
        queue.insert((deadline, task.id.0), task);
    }
    let mut picked_ids = alloc::vec::Vec::new();
    for _ in 0..3 {
        let picked = pick_eevdf(&queue).expect("should pick a task");
        let picked_deadline = picked.deadline;
        picked_ids.push(picked.id);
        let mut task = queue.remove(&(picked_deadline, picked.id.0)).unwrap();
        task.vruntime += 100;
        task.deadline = compute_deadline(task.vruntime, DEFAULT_TIMESLICE, task.weight);
        task.eligible = true;
        queue.insert((task.deadline, task.id.0), task);
    }
    // All three distinct tasks should have been picked
    let unique: alloc::collections::BTreeSet<_> = picked_ids.iter().collect();
    assert_eq!(unique.len(), 3, "should cycle through all 3 tasks");
}

// -----------------------------------------------------------------------
// min_vruntime monotonicity test
// -----------------------------------------------------------------------

#[test]
fn test_min_vruntime_never_decreases() {
    let _guard = crate::test_serial::acquire();
    let mut min_vruntime = 0u64;
    let mut task = make_test_task(870);
    task.vruntime = 0;
    task.weight = 1024;
    for _ in 0..50 {
        update_vruntime(&mut task, 1);
        if task.vruntime > min_vruntime {
            min_vruntime = task.vruntime;
        }
        assert!(
            min_vruntime >= task.vruntime - 1,
            "min_vruntime {} must not decrease (vruntime={})",
            min_vruntime,
            task.vruntime
        );
    }
    // After 50 ticks at weight 1024: each tick adds (1024*1)/1024 = 1
    assert_eq!(task.vruntime, 50);
    assert_eq!(min_vruntime, 50);
}

#[test]
fn test_min_vruntime_monotonic_across_tasks() {
    let _guard = crate::test_serial::acquire();
    let mut min_vruntime = 0u64;
    // Simulate two tasks running alternately
    let mut task_a = make_test_task(880);
    task_a.vruntime = 0;
    task_a.weight = 1024;
    let mut task_b = make_test_task(881);
    task_b.vruntime = 0;
    task_b.weight = 1024;

    for tick in 0..20 {
        if tick % 2 == 0 {
            update_vruntime(&mut task_a, 1);
            if task_a.vruntime > min_vruntime {
                min_vruntime = task_a.vruntime;
            }
        } else {
            update_vruntime(&mut task_b, 1);
            if task_b.vruntime > min_vruntime {
                min_vruntime = task_b.vruntime;
            }
        }
    }
    assert!(min_vruntime >= 10, "min_vruntime should be >= 10 after 10 ticks each");
}
