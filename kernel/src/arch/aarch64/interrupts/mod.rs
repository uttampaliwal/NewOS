pub fn init(_phys_mem_offset: u64) {
    // AArch64: GIC initialization not yet implemented
}

pub fn enable_interrupts() {
    // Enable interrupts on AArch64
}

pub fn disable_interrupts() {
    // Disable interrupts on AArch64
}

pub fn halt() {
    // WFI on AArch64
}

pub fn without_interrupts<F, R>(f: F) -> R 
where 
    F: FnOnce() -> R 
{
    f()
}

pub mod apic {
    pub fn init_per_cpu() {
        // AArch64 uses GIC, so this would be GICv2/v3 initialization
    }
    
    pub fn signal_eoi() {
        // GIC EOI
    }
}
