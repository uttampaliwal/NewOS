use crate::gdt;
use core::arch::global_asm;
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::VirtAddr;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

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

        // Initialize modern APIC with correctly mapped virtual addresses
        let lapic_phys = apic::get_base_addr();
        let lapic_virt = phys_mem_offset + lapic_phys.as_u64();
        
        {
            let mut lapic = LAPIC.lock();
            *lapic = apic::LocalApic::new(lapic_virt);
            lapic.initialize();
            // Start timer with a reasonable count for periodic interrupts
            lapic.start_timer(0x10000);
        }

        // Initialize IOAPIC and route Keyboard (IRQ 1)
        // Standard IOAPIC is at 0xFEC00000 physical
        let ioapic_virt = phys_mem_offset + 0xFEC00000u64;
        {
            let mut ioapic = IOAPIC.lock();
            *ioapic = apic::IoApic::new(ioapic_virt);
            ioapic.route_irq(1, KEYBOARD_INTERRUPT_VECTOR);
        }
    }
}

pub fn init_idt() {
    IDT.load();
}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
global_asm!(
    r#"
    .global timer_interrupt_entry
    timer_interrupt_entry:
        // TRACE: write 'T' to serial port 0x3f8
        push rax
        push rdx
        mov dx, 0x3f8
        mov al, 84 // 'T'
        out dx, al
        pop rdx
        pop rax

        // 1. Swap GS if we came from Ring 3
        // Check CS in the IRETQ frame (RSP+120+8)
        test qword ptr [rsp + 8], 0x3
        jz .timer_no_swap
        swapgs
    .timer_no_swap:

        // 2. Save all registers
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

        // 3. Call the scheduler tick
        mov rdi, rsp
        call timer_interrupt_handler_inner
        
        // 4. Handle stack switch
        mov rsp, rax

    .timer_restore:
        // 5. Restore all registers
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

        // 6. Swap GS back if we came from Ring 3
        test qword ptr [rsp + 8], 0x3
        jz .timer_done
        swapgs
    .timer_done:
        iretq

    .global yield_interrupt_entry
    yield_interrupt_entry:
        test qword ptr [rsp + 8], 0x3
        jz .yield_no_swap
        swapgs
    .yield_no_swap:

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

        // 4. Handle stack switch
        mov rsp, rax

    .yield_restore:
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

        test qword ptr [rsp + 8], 0x3
        jz .yield_done
        swapgs
    .yield_done:
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
    let next_stack = crate::task::scheduler::timer_tick(stack_ptr);
    if next_stack != 0 {
        // Optional: add serial log for context switch
        // crate::serial::print(format_args!("S")); 
    }
    next_stack
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
    use pc_keyboard::DecodedKey;
    use x86_64::instructions::port::Port;
    use crate::input::{KEYBOARD, add_char};

    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);

    let scancode: u8 = unsafe { port.read() };
    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(character) => {
                    add_char(character);
                }
                DecodedKey::RawKey(_) => {}
            }
        }
    }

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
    use x86_64::registers::model_specific::{GsBase, KernelGsBase};
    
    let addr = Cr2::read();

    // Check if the fault occurred in user mode (CS segment selector has RPL 3)
    if stack_frame.code_segment.0 & 0x3 == 0x3 {
        crate::serial::println!("PROCESS FAULT: Page Fault at {:?} with error code {:?}. Terminating process.", addr, error_code);
        crate::task::scheduler::exit_current_task();
    }

    let gs_base = GsBase::read();
    let kernel_gs_base = KernelGsBase::read();

    panic!(
        "EXCEPTION: PAGE FAULT in Kernel\nAccessed Address: {:?}\nError Code: {:?}\nGS_BASE: {:?}, KERNEL_GS_BASE: {:?}\n{:#?}",
        addr,
        error_code,
        gs_base,
        kernel_gs_base,
        stack_frame
    );
}

extern "x86-interrupt" fn gpf_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    use x86_64::registers::model_specific::{GsBase, KernelGsBase};

    if stack_frame.code_segment.0 & 0x3 == 0x3 {
        crate::serial::println!("PROCESS FAULT: General Protection Fault with error code {}. Terminating process.", error_code);
        crate::task::scheduler::exit_current_task();
    }

    let gs_base = GsBase::read();
    let kernel_gs_base = KernelGsBase::read();

    crate::serial::println!("EXCEPTION: GENERAL PROTECTION FAULT in Kernel");
    crate::serial::println!("Error Code: {:?}", error_code);
    crate::serial::println!("GS_BASE: {:?}, KERNEL_GS_BASE: {:?}", gs_base, kernel_gs_base);
    crate::serial::println!("Instruction Pointer: {:?}", stack_frame.instruction_pointer);
    crate::serial::println!("Stack Pointer: {:?}", stack_frame.stack_pointer);
    crate::serial::println!("{:#?}", stack_frame);
    panic!("GPF");
}

extern "x86-interrupt" fn syscall_handler(_stack_frame: InterruptStackFrame) {
    crate::serial::print(format_args!("[syscall int 0x80]\n"));
}
