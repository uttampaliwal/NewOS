use super::{Task, TaskId};
use crate::process::Process;
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
    current_task: Option<Task>,
    task_count: usize,
    current_task_id: Option<TaskId>,
}

impl Scheduler {
    fn new() -> Self {
        Self {
            tasks: VecDeque::new(),
            current_task: None,
            task_count: 0,
            current_task_id: None,
        }
    }
}

pub fn add_task(task: Task) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        sched.tasks.push_back(task);
        sched.task_count += 1;
    });
}

pub fn start_scheduling() -> ! {
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        panic!("scheduler is only available on the freestanding kernel target");
    }

    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let mut next_ptr: usize = 0;

        x86_64::instructions::interrupts::without_interrupts(|| {
            let mut sched = SCHEDULER.lock();

            if let Some(mut next_task) = sched.tasks.pop_front() {
                // Prepare hardware for the next task (TSS, CR3, GS Base)
                next_task.switch_to();
                next_task.state = super::TaskState::Running;
                sched.current_task = Some(next_task);
                sched.current_task_id = sched.current_task.as_ref().map(|t| t.id);
                next_ptr = sched.current_task.as_ref().unwrap().stack_ptr;
            }
        });

        if next_ptr != 0 {
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

    if let Some(mut sched) = SCHEDULER.try_lock() {
        if let Some(mut prev_task) = sched.current_task.take() {
            // Save current stack pointer
            prev_task.stack_ptr = current_stack_ptr;

            // If the task is running, put it back in the queue as ready.
            // If it was blocked or zombie, we don't put it back in the ready queue.
            let was_running = prev_task.state == super::TaskState::Running;
            let is_zombie = prev_task.state == super::TaskState::Zombie;

            if let Some(mut next_task) = sched.tasks.pop_front() {
                // There is a next task to switch to.
                if was_running {
                    prev_task.state = super::TaskState::Ready;
                    sched.tasks.push_back(prev_task);
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
