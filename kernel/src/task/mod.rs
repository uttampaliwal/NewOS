use alloc::boxed::Box;
use core::arch::global_asm;

pub mod scheduler;

global_asm!(
    r#"
    .global switch_context
switch_context:
    // sysv64 ABI: rdi = prev_rsp, rsi = next_rsp
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    // Save current stack pointer to prev_rsp
    mov [rdi], rsp
    // Load next stack pointer from next_rsp
    mov rsp, [rsi]

    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx

    ret
    "#
);

unsafe extern "sysv64" {
    fn switch_context(prev_rsp: *mut usize, next_rsp: *const usize);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(usize);

impl TaskId {
    pub fn new() -> Self {
        use core::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        TaskId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

pub struct Task {
    #[allow(dead_code)] // Will be used when we add preemption or task management
    id: TaskId,
    stack_ptr: usize,
    #[allow(dead_code)] // Kept to ensure the stack is not deallocated
    stack: Box<[u8]>,
}

impl Task {
    pub fn new(entry_point: extern "sysv64" fn()) -> Self {
        const STACK_SIZE: usize = 4096 * 4; // 16 KiB stack
        let mut stack = alloc::vec![0u8; STACK_SIZE].into_boxed_slice();

        let stack_ptr = stack.as_mut_ptr() as usize + STACK_SIZE;
        let mut stack_ptr = stack_ptr as *mut usize;

        unsafe {
            // Setup the initial stack frame that switch_context expects.
            // 1. Return address (entry point)
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(entry_point as usize);

            // 2. Callee-saved registers (RBX, RBP, R12, R13, R14, R15)
            for _ in 0..6 {
                stack_ptr = stack_ptr.sub(1);
                stack_ptr.write(0);
            }
        }

        Self {
            id: TaskId::new(),
            stack_ptr: stack_ptr as usize,
            stack,
        }
    }
}
