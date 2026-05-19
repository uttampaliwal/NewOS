pub fn init() {
    // AArch64: VBAR_EL1 setup for SVC handling not yet implemented
}

#[unsafe(no_mangle)]
pub extern "C" fn syscall_dispatch() {
    // AArch64: SVC syscall dispatch not yet implemented
    loop {
        core::hint::spin_loop();
    }
}
