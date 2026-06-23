use super::{Task, TaskId};
use crate::process::{Process, ProcessId};
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

static UPTIME_TICKS: AtomicU64 = AtomicU64::new(0);

lazy_static! {
    static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
}

struct Scheduler {
    cpu_queues: alloc::vec::Vec<VecDeque<Task>>,
    cpu_current: alloc::vec::Vec<Option<Task>>,
    cpu_current_id: alloc::vec::Vec<Option<TaskId>>,
    blocked_tasks: alloc::vec::Vec<Task>,
    task_count: usize,
}

impl Scheduler {
    fn new() -> Self {
        let cpu_count = crate::smp::get_cpu_count() as usize;
        let mut cpu_queues = alloc::vec::Vec::with_capacity(cpu_count);
        let mut cpu_current = alloc::vec::Vec::with_capacity(cpu_count);
        let mut cpu_current_id = alloc::vec::Vec::with_capacity(cpu_count);
        for _ in 0..cpu_count {
            cpu_queues.push(VecDeque::new());
            cpu_current.push(None);
            cpu_current_id.push(None);
        }
        Self {
            cpu_queues,
            cpu_current,
            cpu_current_id,
            blocked_tasks: alloc::vec![],
            task_count: 0,
        }
    }

    fn current_cpu_id(&self) -> usize {
        crate::smp::get_current_cpu_id() as usize
    }

    /// Find the CPU with the shortest run queue for load balancing.
    fn least_loaded_cpu(&self) -> usize {
        let mut min_len = usize::MAX;
        let mut min_cpu = 0;
        for (cpu, queue) in self.cpu_queues.iter().enumerate() {
            if queue.len() < min_len {
                min_len = queue.len();
                min_cpu = cpu;
            }
        }
        min_cpu
    }

    /// Steal a task from the busiest CPU.
    fn steal_task(&mut self) -> Option<Task> {
        let my_cpu = self.current_cpu_id();
        let mut max_len = 0;
        let mut max_cpu = 0;
        for (cpu, queue) in self.cpu_queues.iter().enumerate() {
            if cpu != my_cpu && queue.len() > max_len {
                max_len = queue.len();
                max_cpu = cpu;
            }
        }
        if max_len > 1 {
            self.cpu_queues[max_cpu].pop_back()
        } else {
            None
        }
    }
}

pub fn add_task(task: Task) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let target = sched.least_loaded_cpu();
        let queue = &mut sched.cpu_queues[target];
        let pos = queue.iter().position(|t| t.priority > task.priority).unwrap_or(queue.len());
        queue.insert(pos, task);
        sched.task_count += 1;
    });
}

pub fn remove_task(task_id: TaskId) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        if let Some(ref current) = sched.cpu_current[cpu]
            && current.id == task_id
        {
            sched.cpu_current[cpu] = None;
            sched.cpu_current_id[cpu] = None;
            sched.task_count -= 1;
            return;
        }
        for queue in &mut sched.cpu_queues {
            let len_before = queue.len();
            queue.retain(|t| t.id != task_id);
            if queue.len() < len_before {
                sched.task_count -= 1;
                return;
            }
        }
    });
}

pub fn start_scheduling() -> ! {
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        panic!("scheduler is only available on the freestanding kernel target");
    }

    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        x86_64::instructions::interrupts::disable();

        let mut sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        if let Some(mut next_task) = sched.cpu_queues[cpu].pop_front() {
            next_task.switch_to();
            next_task.state = super::TaskState::Running;
            sched.cpu_current[cpu] = Some(next_task);
            sched.cpu_current_id[cpu] = sched.cpu_current[cpu].as_ref().map(|t| t.id);
            let next_ptr = match sched.cpu_current[cpu].as_ref() {
                Some(t) => t.stack_ptr,
                None => unreachable!("scheduler: cpu_current was just set to Some"),
            };
            drop(sched);

            unsafe {
                core::arch::asm!(
                    "mov rsp, {0}",
                    "pop r15", "pop r14", "pop r13", "pop r12", "pop r11",
                    "pop r10", "pop r9", "pop r8", "pop rdi", "pop rsi",
                    "pop rbp", "pop rdx", "pop rcx", "pop rbx", "pop rax",
                    "test qword ptr [rsp + 8], 0x3",
                    "jz 2f",
                    "swapgs",
                    "2:",
                    "iretq",
                    in(reg) next_ptr,
                    options(noreturn)
                );
            }
        }

        panic!("No tasks to schedule!");
    }
}

pub fn yield_task() {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    unsafe {
        core::arch::asm!("int {vector}", vector = const crate::interrupts::YIELD_INTERRUPT_VECTOR);
    }
}

pub fn get_current_kernel_stack_top() -> usize {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        sched.cpu_current[cpu]
            .as_ref()
            .map(|t| t.kernel_stack_top)
            .unwrap_or(0)
    })
}

pub fn timer_tick(current_stack_ptr: usize) -> usize {
    UPTIME_TICKS.fetch_add(1, Ordering::Relaxed);

    if let Some(mut sched) = SCHEDULER.try_lock() {
        let cpu = sched.current_cpu_id();
        if let Some(mut prev_task) = sched.cpu_current[cpu].take() {
            prev_task.stack_ptr = current_stack_ptr;

            let was_running = prev_task.state == super::TaskState::Running;
            let is_zombie = prev_task.state == super::TaskState::Zombie;

            // CFS vruntime: increment for NORMAL/BATCH tasks by (1024 / nice_weight).
            // Nice 0 weight = 1024, so vruntime += 1 per tick.
            if was_running {
                match prev_task.policy {
                    super::scheduler_class::SchedulingPolicy::SCHED_NORMAL
                    | super::scheduler_class::SchedulingPolicy::SCHED_BATCH => {
                        prev_task.vruntime = prev_task.vruntime.saturating_add(1);
                    }
                    _ => {}
                }
            }

            let mut should_preempt = false;
            if was_running && prev_task.time_slice > 0 && prev_task.time_slice != u32::MAX {
                prev_task.time_slice -= 1;
                if prev_task.time_slice == 0 {
                    should_preempt = true;
                }
                // Enforce cgroup cpu_max: if quota exceeded, force preempt.
                let pid_val = prev_task.process.id().0 as u32;
                if !crate::cgroup::cgroup_cpu_tick(pid_val) {
                    should_preempt = true;
                }
            }

            if prev_task.policy == super::scheduler_class::SchedulingPolicy::SCHED_FIFO {
                should_preempt = false;
            }

            // CFS: for SCHED_NORMAL, preempt if any queued task has lower vruntime.
            if was_running
                && matches!(
                    prev_task.policy,
                    super::scheduler_class::SchedulingPolicy::SCHED_NORMAL
                        | super::scheduler_class::SchedulingPolicy::SCHED_BATCH
                )
                && let Some(min_vr) = sched.cpu_queues[cpu]
                    .iter()
                    .map(|t| t.vruntime)
                    .min()
                && min_vr < prev_task.vruntime
            {
                should_preempt = true;
            }

            // Try local queue first, then steal from busiest CPU.
            let mut next_task = sched.cpu_queues[cpu].pop_front();
            if next_task.is_none() {
                next_task = sched.steal_task();
            }

            if let Some(mut next_task) = next_task {
                if was_running && (should_preempt || next_task.priority < prev_task.priority) {
                    prev_task.state = super::TaskState::Ready;
                    prev_task.time_slice = super::scheduler_class::default_timeslice(prev_task.policy);
                    let queue = &mut sched.cpu_queues[cpu];
                    let pos = queue.iter().position(|t| t.priority > prev_task.priority).unwrap_or(queue.len());
                    queue.insert(pos, prev_task);
                } else if was_running {
                    sched.cpu_queues[cpu].push_back(next_task);
                    next_task = prev_task;
                }

                next_task.switch_to();
                next_task.state = super::TaskState::Running;
                let next_ptr = next_task.stack_ptr;
                sched.cpu_current[cpu] = Some(next_task);
                sched.cpu_current_id[cpu] = sched.cpu_current[cpu].as_ref().map(|t| t.id);

                return next_ptr;
            } else {
                if is_zombie && sched.task_count == 0 {
                    crate::serial::println!("[scheduler] CPU {}: All tasks exited. Halting.", cpu);
                    loop {
                        x86_64::instructions::hlt();
                    }
                }
                sched.cpu_current[cpu] = Some(prev_task);
                sched.cpu_current_id[cpu] = sched.cpu_current[cpu].as_ref().map(|t| t.id);
            }
        }
    }
    current_stack_ptr
}

pub fn exit_current_task() -> ! {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        if let Some(ref mut task) = sched.cpu_current[cpu] {
            crate::serial::println!("[scheduler] CPU {} Task {} exited/terminated.", cpu, task.id.0);
            task.state = super::TaskState::Zombie;
        }
    });

    yield_task();

    loop {
        x86_64::instructions::hlt();
    }
}

pub fn get_uptime_ticks() -> u64 {
    UPTIME_TICKS.load(Ordering::Relaxed)
}

pub fn get_task_count() -> usize {
    SCHEDULER.lock().task_count
}

pub fn get_current_task_id() -> Option<TaskId> {
    let sched = SCHEDULER.lock();
    let cpu = sched.current_cpu_id();
    sched.cpu_current_id[cpu]
}

pub fn get_current_process() -> Option<Process> {
    let sched = SCHEDULER.lock();
    let cpu = sched.current_cpu_id();
    sched.cpu_current[cpu].as_ref().map(|task| task.process.clone())
}

pub fn with_current_task_mut<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut Task) -> R,
{
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        sched.cpu_current[cpu].as_mut().map(f)
    })
}

pub fn set_current_policy(policy: super::scheduler_class::SchedulingPolicy, priority: u8) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        if let Some(ref mut task) = sched.cpu_current[cpu] {
            task.policy = policy;
            task.priority = priority;
            task.time_slice = super::scheduler_class::default_timeslice(policy);
        }
    });
}

pub fn get_current_policy() -> Option<(super::scheduler_class::SchedulingPolicy, u8)> {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        sched.cpu_current[cpu].as_ref().map(|t| (t.policy, t.priority))
    })
}

pub fn get_current_process_id() -> Option<ProcessId> {
    let sched = SCHEDULER.lock();
    let cpu = sched.current_cpu_id();
    sched.cpu_current[cpu].as_ref().map(|t| t.process.id())
}

pub fn block_current() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        if let Some(mut task) = sched.cpu_current[cpu].take() {
            sched.cpu_current_id[cpu] = None;
            task.state = super::TaskState::Blocked;
            sched.task_count -= 1;
            sched.blocked_tasks.push(task);
        }
    });
}

pub fn wake_task_by_id(task_id: TaskId) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        for i in 0..sched.blocked_tasks.len() {
            if sched.blocked_tasks[i].id == task_id {
                let mut task = sched.blocked_tasks.swap_remove(i);
                task.state = super::TaskState::Ready;
                sched.task_count += 1;
                // Route to least-loaded CPU.
                let target = sched.least_loaded_cpu();
                let queue = &mut sched.cpu_queues[target];
                let pos = queue.iter().position(|t| t.priority > task.priority).unwrap_or(queue.len());
                queue.insert(pos, task);
                return;
            }
        }
    });
}

pub fn block_current_waiting_for_child() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        if let Some(mut task) = sched.cpu_current[cpu].take() {
            sched.cpu_current_id[cpu] = None;
            task.state = super::TaskState::Blocked;
            sched.task_count -= 1;
            sched.blocked_tasks.push(task);
        }
    });
}

pub fn wake_tasks_waiting_for_parent(parent_pid: ProcessId) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let mut i = 0;
        while i < sched.blocked_tasks.len() {
            if sched.blocked_tasks[i].process.id() == parent_pid {
                let mut task = sched.blocked_tasks.swap_remove(i);
                task.state = super::TaskState::Ready;
                sched.task_count += 1;
                let target = sched.least_loaded_cpu();
                let queue = &mut sched.cpu_queues[target];
                let pos = queue.iter().position(|t| t.priority > task.priority).unwrap_or(queue.len());
                queue.insert(pos, task);
            } else {
                i += 1;
            }
        }
    });
}

#[cfg(test)]
pub fn set_current_task_for_test(task: Task) {
    let mut sched = SCHEDULER.lock();
    let cpu = sched.current_cpu_id();
    sched.cpu_current_id[cpu] = Some(task.id);
    sched.cpu_current[cpu] = Some(task);
}

#[cfg(test)]
pub fn test_reset() {
    let mut sched = SCHEDULER.lock();
    let cpu = sched.current_cpu_id();
    sched.cpu_current[cpu] = None;
    sched.cpu_current_id[cpu] = None;
    sched.blocked_tasks.clear();
    for queue in &mut sched.cpu_queues {
        queue.clear();
    }
    sched.task_count = 0;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{Process, ProcessControlBlock, ProcessId, ProcessState, SignalAction, SignalSet};
    use crate::task::{Task, TaskState};
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
        };
        Process {
            inner: Arc::new(Mutex::new(pcb)),
        }
    }

    fn make_test_task(pid: usize) -> Task {
        Task::new_test(TaskId::new(), make_test_process(pid), TaskState::Running)
    }

    // -- Query function tests (no without_interrupts, safe in test mode) --

    #[test]
    fn test_get_task_count_initial_zero() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        assert_eq!(get_task_count(), 0);
    }

    #[test]
    fn test_get_current_task_id_none_when_empty() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        assert!(get_current_task_id().is_none());
    }

    #[test]
    fn test_set_current_task_then_get_id() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let task = make_test_task(42);
        let expected_id = task.id;
        set_current_task_for_test(task);
        assert_eq!(get_current_task_id(), Some(expected_id));
    }

    #[test]
    fn test_get_current_process_after_set() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let task = make_test_task(77);
        let pid = task.process.id();
        set_current_task_for_test(task);
        let proc = get_current_process();
        assert!(proc.is_some());
        assert_eq!(proc.unwrap().id(), pid);
    }

    #[test]
    fn test_get_current_process_none_when_empty() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        assert!(get_current_process().is_none());
    }

    #[test]
    fn test_get_current_process_id() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let task = make_test_task(55);
        set_current_task_for_test(task);
        assert_eq!(get_current_process_id(), Some(ProcessId(55)));
    }

    #[test]
    fn test_get_current_process_id_none_when_empty() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        assert!(get_current_process_id().is_none());
    }

    // Note: tests for get_current_kernel_stack_top are omitted because
    // the function uses without_interrupts (cli/sti) which SIGSEGVs in
    // userspace test mode.

    #[test]
    fn test_get_uptime_ticks_initial() {
        let _guard = crate::test_serial::acquire();
        let _ticks = get_uptime_ticks();
    }

    #[test]
    fn test_test_reset_clears_current() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let task = make_test_task(1);
        set_current_task_for_test(task);
        assert!(get_current_task_id().is_some());
        test_reset();
        assert!(get_current_task_id().is_none());
    }

    #[test]
    fn test_test_reset_clears_task_count() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        // task_count is manipulated by blocked_tasks and queues;
        // after test_reset it must be 0.
        assert_eq!(get_task_count(), 0);
    }

    #[test]
    fn test_set_current_task_replaces_previous() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let task1 = make_test_task(10);
        let id1 = task1.id;
        set_current_task_for_test(task1);
        let task2 = make_test_task(20);
        let id2 = task2.id;
        set_current_task_for_test(task2);
        assert_eq!(get_current_task_id(), Some(id2));
        assert_ne!(get_current_task_id(), Some(id1));
    }

    #[test]
    fn test_process_id_matches_after_set() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let task = make_test_task(123);
        set_current_task_for_test(task);
        assert_eq!(get_current_process_id(), Some(ProcessId(123)));
        assert_eq!(get_current_task_count(), 0);
    }

    fn get_current_task_count() -> usize {
        get_task_count()
    }
}
