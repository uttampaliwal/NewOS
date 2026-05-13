pub fn init() {
    panic!("AArch64 GDT equivalent (EL1 vector table) not yet implemented");
}

pub fn init_for_cpu(_cpu_id: u32) {
    panic!("AArch64 GDT equivalent (EL1 vector table) not yet implemented");
}

pub fn set_interrupt_stack(_stack_top: u64) {
    panic!("AArch64 set_interrupt_stack not yet implemented");
}

pub fn reload_gdt() {
    // No-op on AArch64 or equivalent to reloading VBAR_EL1
}
