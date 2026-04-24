use super::{Task, switch_context};
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
    SCHEDULER.lock().tasks.push_back(task);
}

pub fn start_scheduling() -> ! {
    let mut sched = SCHEDULER.lock();

    if let Some(next_task) = sched.tasks.pop_front() {
        sched.current_task = Some(next_task);

        let next_ptr = &sched.current_task.as_ref().unwrap().stack_ptr as *const usize;
        let prev_ptr = &mut sched.boot_stack_ptr as *mut usize;

        // Unlock the scheduler before switching context
        drop(sched);

        unsafe {
            switch_context(prev_ptr, next_ptr);
        }

        // When we return to boot task (if ever), we would end up here.
        // For now, we don't expect to return.
        panic!("Returned to boot task unexpectedly!");
    }

    panic!("No tasks to schedule!");
}

pub fn yield_task() {
    let mut sched = SCHEDULER.lock();

    let prev_task = sched
        .current_task
        .take()
        .expect("yield_task: no current task");

    if let Some(next_task) = sched.tasks.pop_front() {
        sched.tasks.push_back(prev_task);
        sched.current_task = Some(next_task);

        // prev_task is now at the back of the queue
        let prev_ptr = &mut sched.tasks.back_mut().unwrap().stack_ptr as *mut usize;
        let next_ptr = &sched.current_task.as_ref().unwrap().stack_ptr as *const usize;

        // Unlock before switching
        drop(sched);

        unsafe {
            switch_context(prev_ptr, next_ptr);
        }
    } else {
        // No other task, just keep running the current one
        sched.current_task = Some(prev_task);
    }
}
