pub mod handler;

pub fn init() {
    crate::arch::syscall_arch::init();
}

#[unsafe(no_mangle)]
pub extern "C" fn get_current_kernel_stack_top() -> usize {
    crate::task::scheduler::get_current_kernel_stack_top()
}
