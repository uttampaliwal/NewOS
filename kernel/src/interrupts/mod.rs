use crate::gdt;
use core::arch::global_asm;
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use x86_64::VirtAddr;

pub mod apic;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;
pub const SYSCALL_VECTOR: u8 = 0x80;
pub const TIMER_INTERRUPT_VECTOR: u8 = 32;
pub const KEYBOARD_INTERRUPT_VECTOR: u8 = 33;
pub const YIELD_INTERRUPT_VECTOR: u8 = 0x81;

pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

lazy_static! {
    pub static ref LAPIC: Mutex<apic::LocalApic> =
        Mutex::new(unsafe { apic::LocalApic::new(apic::get_base_addr()) });
    
    pub static ref IOAPIC: Mutex<apic::IoApic> =
        Mutex::new(unsafe { apic::IoApic::new(VirtAddr::zero()) });
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = TIMER_INTERRUPT_VECTOR,
}

impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }
}

pub fn init(phys_mem_offset: VirtAddr) {
    init_idt();
    unsafe {
        // Disable legacy PIC
        PICS.lock().disable();

        // Initialize modern APIC
        let mut lapic = LAPIC.lock();
        lapic.initialize();
        // Start timer with a reasonable count for periodic interrupts
        lapic.start_timer(0x10000);

        // Initialize IOAPIC and route Keyboard (IRQ 1)
        // We use the direct physical map offset
        let mut ioapic = apic::IoApic::new(phys_mem_offset + 0xFEC00000u64);
        ioapic.route_irq(1, KEYBOARD_INTERRUPT_VECTOR);
    }
}

pub fn init_idt() {
    IDT.load();
}

global_asm!(
    r#"
    .global timer_interrupt_entry
    timer_interrupt_entry:
        // 1. Save all registers
        push rax
        push rbx
        push rcx
        push rdx
        push rbp
        push rsi
        push rdi
        push r8
        push r9
        push r10
        push r11
        push r12
        push r13
        push r14
        push r15

        // 2. Call the scheduler tick
        mov rdi, rsp
        call timer_interrupt_handler_inner
        
        // 3. Handle stack switch
        cmp rax, 0
        je .no_switch_timer
        mov rsp, rax

    .no_switch_timer:
        // 4. Restore all registers
        pop r15
        pop r14
        pop r13
        pop r12
        pop r11
        pop r10
        pop r9
        pop r8
        pop rdi
        pop rsi
        pop rbp
        pop rdx
        pop rcx
        pop rbx
        pop rax

        iretq

    .global yield_interrupt_entry
    yield_interrupt_entry:
        push rax
        push rbx
        push rcx
        push rdx
        push rbp
        push rsi
        push rdi
        push r8
        push r9
        push r10
        push r11
        push r12
        push r13
        push r14
        push r15

        mov rdi, rsp
        call yield_interrupt_handler_inner

        cmp rax, 0
        je .no_switch_yield
        mov rsp, rax

    .no_switch_yield:
        pop r15
        pop r14
        pop r13
        pop r12
        pop r11
        pop r10
        pop r9
        pop r8
        pop rdi
        pop rsi
        pop rbp
        pop rdx
        pop rcx
        pop rbx
        pop rax
        iretq
    "#
);

unsafe extern "C" {
    fn timer_interrupt_entry();
    fn yield_interrupt_entry();
}

#[unsafe(no_mangle)]
pub extern "C" fn timer_interrupt_handler_inner(stack_ptr: usize) -> usize {
    unsafe {
        LAPIC.lock().signal_eoi();
    }
    crate::task::scheduler::timer_tick(stack_ptr)
}

#[unsafe(no_mangle)]
pub extern "C" fn yield_interrupt_handler_inner(stack_ptr: usize) -> usize {
    crate::task::scheduler::timer_tick(stack_ptr)
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault.set_handler_fn(gpf_handler);

        // Use our custom assembly entry point for the timer
        unsafe {
            let entry_ptr = timer_interrupt_entry as *const ();
            idt[InterruptIndex::Timer.as_u8()].set_handler_fn(core::mem::transmute(entry_ptr));

            let yield_entry_ptr = yield_interrupt_entry as *const ();
            idt[YIELD_INTERRUPT_VECTOR as u8].set_handler_fn(core::mem::transmute(yield_entry_ptr));
        }

        idt[KEYBOARD_INTERRUPT_VECTOR as u8].set_handler_fn(keyboard_interrupt_handler);
        idt[SYSCALL_VECTOR].set_handler_fn(syscall_handler);
        idt
    };
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    crate::serial::print(format_args!("[kbd] scancode: 0x{:x}\n", scancode));

    unsafe {
        LAPIC.lock().signal_eoi();
    }
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    crate::serial::print(format_args!("EXCEPTION: BREAKPOINT\n{:#?}\n", stack_frame));
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    panic!(
        "EXCEPTION: PAGE FAULT\nAccessed Address: {:?}\nError Code: {:?}\n{:#?}",
        Cr2::read(),
        error_code,
        stack_frame
    );
}

extern "x86-interrupt" fn gpf_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    crate::serial::println!("EXCEPTION: GENERAL PROTECTION FAULT");
    crate::serial::println!("Error Code: {:?}", error_code);
    crate::serial::println!("Instruction Pointer: {:?}", stack_frame.instruction_pointer);
    crate::serial::println!("Stack Pointer: {:?}", stack_frame.stack_pointer);
    crate::serial::println!("{:#?}", stack_frame);
    panic!("GPF");
}

extern "x86-interrupt" fn syscall_handler(_stack_frame: InterruptStackFrame) {
    crate::serial::print(format_args!("[syscall int 0x80]\n"));
}
