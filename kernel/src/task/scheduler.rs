use super::{Task, TaskId};
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
                "iretq",
                in(reg) next_ptr,
                options(noreturn)
            );
        }
    }

    panic!("No tasks to schedule!");
}

pub fn yield_task() {
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
            if let Some(mut next_task) = sched.tasks.pop_front() {
                // Prepare hardware for the next task
                next_task.switch_to();
                next_task.state = super::TaskState::Running;
                prev_task.state = super::TaskState::Ready;
                prev_task.stack_ptr = current_stack_ptr;
                sched.tasks.push_back(prev_task);
                sched.current_task = Some(next_task);
                sched.current_task_id = sched.current_task.as_ref().map(|t| t.id);

                return sched.current_task.as_ref().unwrap().stack_ptr;
            } else {
                sched.current_task = Some(prev_task);
            }
        }
    }
    0
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
