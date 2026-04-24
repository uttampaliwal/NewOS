use alloc::alloc::{Layout, alloc, dealloc};

pub mod scheduler;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Zombie,
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
    #[allow(dead_code)]
    id: TaskId,
    pub(crate) stack_ptr: usize,
    pub(crate) kernel_stack_top: usize,
    state: TaskState,
    stack_base: *mut u8,
    stack_size: usize,
}

impl Task {
    pub fn new(entry_point: extern "sysv64" fn()) -> Self {
        const STACK_SIZE: usize = 4096 * 4; // 16 KiB stack
        
        let layout = Layout::from_size_align(STACK_SIZE, 16).unwrap();
        let stack_base = unsafe { alloc(layout) };
        if stack_base.is_null() {
            panic!("failed to allocate task stack");
        }

        let stack_top = stack_base as usize + STACK_SIZE;
        let mut stack_ptr = stack_top as *mut usize;

        unsafe {
            // Setup the stack to look like an interrupted state for iretq and pop-all-regs
            
            // 1. IRETQ Frame (pushed by CPU)
            // SS
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(0); 
            // RSP
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(stack_top);
            // RFLAGS
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(0x202); // Interrupts enabled
            // CS
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(0x08);
            // RIP
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(entry_point as usize);

            // 2. All general purpose registers (popped by assembly stub)
            // RAX, RBX, RCX, RDX, RBP, RSI, RDI, R8-R15 (15 registers)
            for _ in 0..15 {
                stack_ptr = stack_ptr.sub(1);
                stack_ptr.write(0);
            }
        }

        Self {
            id: TaskId::new(),
            stack_ptr: stack_ptr as usize,
            kernel_stack_top: stack_top,
            state: TaskState::Ready,
            stack_base,
            stack_size: STACK_SIZE,
        }
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(self.stack_size, 16).unwrap();
        unsafe {
            dealloc(self.stack_base, layout);
        }
    }
}

/// Transition from Ring 0 to Ring 3.
/// 
/// SAFETY: The entry point must be a valid user-mode address.
pub unsafe fn jump_to_user(entry_point: usize) -> ! {
    const USER_STACK_SIZE: usize = 4096 * 4;
    let layout = Layout::from_size_align(USER_STACK_SIZE, 16).unwrap();
    let user_stack_base = unsafe { alloc(layout) };
    let user_stack_top = user_stack_base as usize + USER_STACK_SIZE;

    // We use the selectors defined in GDT.
    // User Data: 0x23 (Index 4, RPL 3)
    // User Code: 0x1b (Index 3, RPL 3)
    let user_data_selector: u64 = 0x23;
    let user_code_selector: u64 = 0x1b;

    unsafe {
        core::arch::asm!(
            "push {stack_seg}",
            "push {stack_ptr}",
            "push 0x202", // RFLAGS: interrupts enabled
            "push {code_seg}",
            "push {entry_ptr}",
            "iretq",
            stack_seg = in(reg) user_data_selector,
            stack_ptr = in(reg) user_stack_top,
            code_seg = in(reg) user_code_selector,
            entry_ptr = in(reg) entry_point,
            options(noreturn)
        );
    }
}
