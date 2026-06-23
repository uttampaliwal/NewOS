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
    tasks: VecDeque<Task>,
    /// Tasks that are blocked waiting for *any* child to exit.
    blocked_tasks: alloc::vec::Vec<Task>,
    current_task: Option<Task>,
    task_count: usize,
    current_task_id: Option<TaskId>,
}

impl Scheduler {
    fn new() -> Self {
        Self {
            tasks: VecDeque::new(),
            blocked_tasks: alloc::vec![],
            current_task: None,
            task_count: 0,
            current_task_id: None,
        }
    }
}

pub fn add_task(task: Task) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let pos = sched.tasks.iter().position(|t| t.priority > task.priority).unwrap_or(sched.tasks.len());
        sched.tasks.insert(pos, task);
        sched.task_count += 1;
    });
}

pub fn remove_task(task_id: TaskId) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        if let Some(ref current) = sched.current_task
            && current.id == task_id
        {
            sched.current_task = None;
            sched.current_task_id = None;
            sched.task_count -= 1;
            return;
        }
        let len_before = sched.tasks.len();
        sched.tasks.retain(|t| t.id != task_id);
        if sched.tasks.len() < len_before {
            sched.task_count -= 1;
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
        // Disable interrupts manually; they will be re-enabled by iretq.
        x86_64::instructions::interrupts::disable();

        let mut sched = SCHEDULER.lock();
        if let Some(mut next_task) = sched.tasks.pop_front() {
            next_task.switch_to();
            next_task.state = super::TaskState::Running;
            sched.current_task = Some(next_task);
            sched.current_task_id = sched.current_task.as_ref().map(|t| t.id);
            let next_ptr = sched.current_task.as_ref().unwrap().stack_ptr;
            drop(sched);

            unsafe {
                core::arch::asm!(
                    "mov rsp, {0}",
                    "pop r15", "pop r14", "pop r13", "pop r12", "pop r11",
                    "pop r10", "pop r9", "pop r8", "pop rdi", "pop rsi",
                    "pop rbp", "pop rdx", "pop rcx", "pop rbx", "pop rax",

                    // Check if we are returning to user mode (CS is at [RSP + 8])
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
        sched
            .current_task
            .as_ref()
            .map(|t| t.kernel_stack_top)
            .unwrap_or(0)
    })
}

pub fn timer_tick(current_stack_ptr: usize) -> usize {
    // Increment uptime counter (each tick represents ~10ms if LAPIC is configured that way)
    UPTIME_TICKS.fetch_add(1, Ordering::Relaxed);

    if let Some(mut sched) = SCHEDULER.try_lock()
        && let Some(mut prev_task) = sched.current_task.take()
    {
        // Save current stack pointer
        prev_task.stack_ptr = current_stack_ptr;

        let was_running = prev_task.state == super::TaskState::Running;
        let is_zombie = prev_task.state == super::TaskState::Zombie;

        // Decrement time slice for running tasks.
        let mut should_preempt = false;
        if was_running && prev_task.time_slice > 0 && prev_task.time_slice != u32::MAX {
            prev_task.time_slice -= 1;
            if prev_task.time_slice == 0 {
                should_preempt = true;
            }
        }

        // SCHED_FIFO tasks run until they block/yield; don't preempt on time slice expiry.
        if prev_task.policy == super::scheduler_class::SchedulingPolicy::SCHED_FIFO {
            should_preempt = false;
        }

        if let Some(mut next_task) = sched.tasks.pop_front() {
            // There is a next task to switch to.
            // Only re-queue the previous task if it was preempted (time slice expired)
            // or if a higher-priority task is available.
            if was_running && (should_preempt || next_task.priority < prev_task.priority) {
                prev_task.state = super::TaskState::Ready;
                prev_task.time_slice = super::scheduler_class::default_timeslice(prev_task.policy);
                // Insert in priority order (lower number = higher priority).
                let pos = sched.tasks.iter().position(|t| t.priority > prev_task.priority).unwrap_or(sched.tasks.len());
                sched.tasks.insert(pos, prev_task);
            } else if was_running {
                // Put back the previous task at front, and push next_task to back.
                sched.tasks.push_back(next_task);
                next_task = prev_task;
            }

            // Prepare hardware for the next task
            next_task.switch_to();
            next_task.state = super::TaskState::Running;
            let next_ptr = next_task.stack_ptr;
            sched.current_task = Some(next_task);
            sched.current_task_id = sched.current_task.as_ref().map(|t| t.id);

            return next_ptr;
        } else {
            // No other tasks - check if we should halt
            if is_zombie {
                // Last task exited - halt the system
                crate::serial::println!("[scheduler] All tasks exited. Halting system.");
                loop {
                    x86_64::instructions::hlt();
                }
            }
            // No tasks to run, return to current context
            sched.current_task = Some(prev_task);
            sched.current_task_id = sched.current_task.as_ref().map(|t| t.id);
        }
    }
    current_stack_ptr
}

pub fn exit_current_task() -> ! {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        if let Some(task) = &mut sched.current_task {
            crate::serial::println!("[scheduler] Task {} exited/terminated.", task.id.0);
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
    SCHEDULER.lock().current_task_id
}

pub fn get_current_process() -> Option<Process> {
    let sched = SCHEDULER.lock();
    sched.current_task.as_ref().map(|task| task.process.clone())
}

pub fn with_current_task_mut<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut Task) -> R,
{
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        sched.current_task.as_mut().map(f)
    })
}

pub fn set_current_policy(policy: super::scheduler_class::SchedulingPolicy, priority: u8) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        if let Some(ref mut task) = sched.current_task {
            task.policy = policy;
            task.priority = priority;
            task.time_slice = super::scheduler_class::default_timeslice(policy);
        }
    });
}

pub fn get_current_policy() -> Option<(super::scheduler_class::SchedulingPolicy, u8)> {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let sched = SCHEDULER.lock();
        sched.current_task.as_ref().map(|t| (t.policy, t.priority))
    })
}

pub fn get_current_process_id() -> Option<ProcessId> {
    SCHEDULER
        .lock()
        .current_task
        .as_ref()
        .map(|t| t.process.id())
}

/// Block the current task unconditionally.
///
/// The task's state is set to `Blocked` and it is moved off the run queue
/// into `blocked_tasks`.  The caller must immediately yield after this
/// returns so the scheduler can switch to another task.
/// The task will be woken by `wake_task_by_id`.
pub fn block_current() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        if let Some(mut task) = sched.current_task.take() {
            sched.current_task_id = None;
            task.state = super::TaskState::Blocked;
            sched.task_count -= 1;
            sched.blocked_tasks.push(task);
        }
    });
}

/// Wake a specific task by its TaskId, moving it from `blocked_tasks` to the
/// ready queue.
pub fn wake_task_by_id(task_id: TaskId) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        for i in 0..sched.blocked_tasks.len() {
            if sched.blocked_tasks[i].id == task_id {
                let mut task = sched.blocked_tasks.swap_remove(i);
                task.state = super::TaskState::Ready;
                sched.task_count += 1;
                sched.tasks.push_back(task);
                return;
            }
        }
    });
}

/// Block the current task until *any* child process of `parent_pid` exits.
///
/// The task's state is set to `Blocked` and it is moved off the run queue
/// into `blocked_tasks`.  The caller must immediately yield after this
/// returns so the scheduler can switch to another task.
pub fn block_current_waiting_for_child() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        if let Some(mut task) = sched.current_task.take() {
            sched.current_task_id = None;
            task.state = super::TaskState::Blocked;
            sched.task_count -= 1; // no longer in the runnable count
            sched.blocked_tasks.push(task);
        }
    });
}

/// Wake every task that is blocked waiting for children of `parent_pid`.
///
/// Called from `exit_current_task` / `handle_exit` after the process has
/// transitioned to `Zombie`.
pub fn wake_tasks_waiting_for_parent(parent_pid: ProcessId) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let mut i = 0;
        while i < sched.blocked_tasks.len() {
            // Wake any blocked task whose process is the parent of the zombie.
            if sched.blocked_tasks[i].process.id() == parent_pid {
                let mut task = sched.blocked_tasks.swap_remove(i);
                task.state = super::TaskState::Ready;
                sched.task_count += 1;
                sched.tasks.push_back(task);
                // Don't advance i — the swap moved a different element here.
            } else {
                i += 1;
            }
        }
    });
}

#[cfg(test)]
pub fn set_current_task_for_test(task: Task) {
    let mut sched = SCHEDULER.lock();
    sched.current_task_id = Some(task.id);
    sched.current_task = Some(task);
}

/// Reset scheduler state to clean between tests.
#[cfg(test)]
pub fn test_reset() {
    let mut sched = SCHEDULER.lock();
    sched.current_task = None;
    sched.current_task_id = None;
    sched.blocked_tasks.clear();
    sched.tasks.clear();
    sched.task_count = 0;
}
