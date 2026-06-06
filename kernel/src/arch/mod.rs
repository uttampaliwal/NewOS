// kernel/src/arch/mod.rs

#[cfg(feature = "arch-aarch64")]
pub mod aarch64;
#[cfg(feature = "arch-x86_64")]
pub mod x86_64;

/// Errors returned by arch stub implementations.
#[derive(Debug)]
pub enum ArchError {
    NotImplemented,
    HardwareFault(&'static str),
}

/// Trait covering all architecture-specific kernel services.
/// Note: This is intended for future use when we might want more dynamic
/// dispatch or just as a formal specification. Currently, we use compile-time
/// feature selection and re-exports.
pub trait ArchInterface {
    // --- GDT / Segment setup ---
    fn init_gdt() -> Result<(), ArchError>;
    fn init_gdt_for_cpu(cpu_id: u32) -> Result<(), ArchError>;
    fn set_interrupt_stack(stack_top: u64) -> Result<(), ArchError>;

    // --- IDT / Interrupt controller ---
    fn init_interrupts(phys_mem_offset: u64) -> Result<(), ArchError>;
    fn enable_interrupts();
    fn disable_interrupts();
    fn without_interrupts<F, R>(f: F) -> R
    where
        F: FnOnce() -> R;
    fn halt();

    // --- SYSCALL/SYSRET ---
    fn init_syscall() -> Result<(), ArchError>;

    // --- MSR (Specific to x86, but kept for interface completeness or mapped to ELs on ARM) ---
    fn read_msr(msr: u32) -> Result<u64, ArchError>;
    fn write_msr(msr: u32, value: u64) -> Result<(), ArchError>;

    // --- LAPIC ---
    fn init_lapic_for_cpu() -> Result<(), ArchError>;
    fn signal_eoi() -> Result<(), ArchError>;
}

// Re-export the active arch's concrete modules so callers keep using crate::gdt::*
#[cfg(feature = "arch-x86_64")]
pub use x86_64::{context, gdt, interrupts, syscall_arch};

#[cfg(feature = "arch-aarch64")]
pub use aarch64::{context, gdt, interrupts, syscall_arch};
