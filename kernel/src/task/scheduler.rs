use super::Task;
use alloc::collections::VecDeque;
use lazy_static::lazy_static;
use spin::Mutex;

lazy_static! {
    static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
}

struct Scheduler {
    tasks: VecDeque<Task>,
    current_task: Option<Task>,
    boot_stack_ptr: usize,
}

impl Scheduler {
    fn new() -> Self {
        Self {
            tasks: VecDeque::new(),
            current_task: None,
            boot_stack_ptr: 0,
        }
    }
}

pub fn add_task(task: Task) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        SCHEDULER.lock().tasks.push_back(task);
    });
}

pub fn start_scheduling() -> ! {
    let mut next_ptr: usize = 0;

    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();

        if let Some(next_task) = sched.tasks.pop_front() {
            sched.current_task = Some(next_task);
            next_ptr = sched.current_task.as_ref().unwrap().stack_ptr;
        }
    });

    if next_ptr != 0 {
        // Update TSS with the kernel stack top of the first task
        x86_64::instructions::interrupts::without_interrupts(|| {
            let sched = SCHEDULER.lock();
            let kernel_stack_top = sched.current_task.as_ref().unwrap().kernel_stack_top;
            crate::gdt::set_interrupt_stack(x86_64::VirtAddr::new(kernel_stack_top as u64));
        });

        unsafe {
            core::arch::asm!(
                "mov rsp, {0}",
                "pop r15",
                "pop r14",
                "pop r13",
                "pop r12",
                "pop r11",
                "pop r10",
                "pop r9",
                "pop r8",
                "pop rdi",
                "pop rsi",
                "pop rbp",
                "pop rdx",
                "pop rcx",
                "pop rbx",
                "pop rax",
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
        core::arch::asm!("int 32");
    }
}

pub fn timer_tick(current_stack_ptr: usize) -> usize {
    // Only try to yield if we can get the lock, to avoid deadlocks in interrupt handler
    if let Some(mut sched) = SCHEDULER.try_lock() {
        if let Some(mut prev_task) = sched.current_task.take() {
            if let Some(next_task) = sched.tasks.pop_front() {
                // Save the current stack pointer
                prev_task.stack_ptr = current_stack_ptr;

                // Put previous task back in queue
                sched.tasks.push_back(prev_task);

                // Set current task to the next one
                sched.current_task = Some(next_task);

                // Update TSS with the kernel stack top of the new task
                let kernel_stack_top = sched.current_task.as_ref().unwrap().kernel_stack_top;
                crate::gdt::set_interrupt_stack(x86_64::VirtAddr::new(kernel_stack_top as u64));

                // Return the new stack pointer to the assembly stub

                return sched.current_task.as_ref().unwrap().stack_ptr;
            } else {
                // No other task, put it back and return 0 (no switch)
                sched.current_task = Some(prev_task);
                return 0;
            }
        }
    }
    0
}
