pub fn init() {
    // AArch64: VBAR_EL1 vector table initialization not yet implemented
}

pub fn init_for_cpu(_cpu_id: u32) {
    // AArch64: per-CPU VBAR_EL1 not yet implemented
}

pub fn set_interrupt_stack(_stack_top: u64) {
    // AArch64: SP_ELx stack setup not yet implemented
}

pub fn reload_gdt() {
    // AArch64: equivalent to reloading VBAR_EL1
}
