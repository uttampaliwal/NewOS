pub fn init() {
    // Set up VBAR_EL1 or equivalent for SVC handling
}

#[unsafe(no_mangle)]
pub extern "C" fn syscall_dispatch() {
    panic!("AArch64 syscall_dispatch not yet implemented");
}
