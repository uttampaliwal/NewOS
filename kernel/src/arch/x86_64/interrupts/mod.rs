use crate::gdt;
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::VirtAddr;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
use core::arch::global_asm;

pub mod apic;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;
pub const SYSCALL_VECTOR: u8 = 0x80;
pub const TIMER_INTERRUPT_VECTOR: u8 = 32;
pub const KEYBOARD_INTERRUPT_VECTOR: u8 = 33;
pub const SCI_INTERRUPT_VECTOR: u8 = 0x44;
pub const ACPI_PCI_VECTOR_BASE: u8 = 0x50;
pub const ACPI_PCI_VECTOR_COUNT: u8 = 32;
pub const YIELD_INTERRUPT_VECTOR: u8 = 0x81;

// SAFETY: PIC_1_OFFSET and PIC_2_OFFSET are valid ISA PIC configuration
// values. This is initialized during early boot on the BSP with no
// concurrent access.
pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// HHDM-mapped LAPIC base address, set during init().
/// The interrupt handler uses this directly to avoid relying on
/// lazy_static replacement which may not persist across CR3 switches.
static LAPIC_BASE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Write to a LAPIC register using the HHDM-mapped address.
/// This is used by the interrupt handler to avoid page faults when
/// running with a user process page table.
#[inline(always)]
unsafe fn lapic_write(offset: u32, value: u32) {
    let base = LAPIC_BASE.load(core::sync::atomic::Ordering::Relaxed);
    if base != 0 {
        unsafe {
            let ptr = (base + offset as u64) as *mut u32;
            core::ptr::write_volatile(ptr, value);
        }
    }
}

/// Read from a LAPIC register using the HHDM-mapped address.
#[inline(always)]
unsafe fn lapic_read(offset: u32) -> u32 {
    let base = LAPIC_BASE.load(core::sync::atomic::Ordering::Relaxed);
    if base != 0 {
        unsafe {
            let ptr = (base + offset as u64) as *const u32;
            core::ptr::read_volatile(ptr)
        }
    } else {
        0
    }
}

lazy_static! {
    // SAFETY: The LAPIC base address is set during init() with the correct
    // HHDM-mapped virtual address. Before init(), no code should access this.
    pub static ref LAPIC: Mutex<apic::LocalApic> =
        Mutex::new(unsafe { apic::LocalApic::new(VirtAddr::new(0)) });
    // SAFETY: This is a placeholder initialization; the actual IOAPIC address
    // is set later in init(). The zero address is replaced before use.
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
    // SAFETY: This runs during boot on the BSP with interrupts disabled.
    // phys_mem_offset maps valid MMIO regions for LAPIC and IOAPIC hardware.
    // The PIC is disabled and LAPIC/IOAPIC are initialized with correct
    // virtual addresses derived from the physical memory offset.
    unsafe {
        // Disable legacy PIC
        PICS.lock().disable();

        // Initialize modern APIC with correctly mapped virtual addresses
        let lapic_phys = apic::get_base_addr();
        let lapic_virt = phys_mem_offset + lapic_phys.as_u64();

        // Store the HHDM-mapped address so the timer interrupt handler can
        // use it directly without needing to lock the lazy_static LAPIC,
        // which may not work after a CR3 switch to a user process page table.
        LAPIC_BASE.store(lapic_virt.as_u64(), core::sync::atomic::Ordering::Release);

        {
            let mut lapic = LAPIC.lock();
            *lapic = apic::LocalApic::new(lapic_virt);
            lapic.initialize();
            // Start timer with a larger count to avoid interrupt storms
            // (timer fires while handler runs, pending on iretq return)
            lapic.start_timer(0x100000);
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
    // SAFETY: We are inside an interrupt handler. We use LAPIC_BASE (set
    // during init()) to write the EOI register directly instead of going
    // through LAPIC.lock(), which can cause page faults when running with
    // a user process page table (the lazy_static LAPIC may hold a
    // non-HHDM address that isn't mapped in user PML4).
    unsafe {
        lapic_write(0xB0, 0);
    }
    // Read saved CS from the CPU interrupt frame.
    // After 15 saved GPRs (120 bytes), the CPU frame is:
    //   RIP(8), CS(8), RFLAGS(8), RSP(8), SS(8)
    // CS is at stack_ptr + 128.
    // SAFETY: stack_ptr points to the interrupt stack frame saved by the
    // assembly entry point. Offset 128 is the CS field (15 GPRs × 8 bytes +
    // RIP × 8 bytes). The pointer is valid for this single read.
    let cs = unsafe { core::ptr::read_volatile((stack_ptr + 128) as *const u64) as u16 };
    if cs & 0x3 == 0x3 {
        // Came from user mode — preemption allowed.
        // ── DIAGNOSTIC: trace the first few user-mode timer interrupts ─
        static USER_TIMER_COUNT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
        let count = USER_TIMER_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if count < 8 {
            let user_rip = unsafe { core::ptr::read_volatile((stack_ptr + 120) as *const u64) };
            let user_rsp = unsafe { core::ptr::read_volatile((stack_ptr + 144) as *const u64) };
            crate::serial::println!(
                "[TIMER] user_tick #{}: rip={:#x} rsp={:#x} cs={:#x}",
                count, user_rip, user_rsp, cs,
            );
        }
        // Network polling is safe here because user code cannot hold kernel locks.
        #[cfg(feature = "arch-x86_64")]
        crate::net::smoltcp_iface::poll_stack();
        crate::task::scheduler::timer_tick(stack_ptr)
    } else {
        // Came from kernel mode — do not preempt kernel tasks.
        stack_ptr
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn yield_interrupt_handler_inner(stack_ptr: usize) -> usize {
    crate::task::scheduler::timer_tick(stack_ptr)
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        // SAFETY: We are configuring the IDT during single-threaded
        // initialization. The IST index references a valid GDT stack.
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault.set_handler_fn(gpf_handler);

        // Use our custom assembly entry point for the timer
        // SAFETY: The assembly entry points (timer_interrupt_entry,
        // yield_interrupt_entry) follow the x86-interrupt calling convention.
        // The transmute converts raw function pointers to the IDT handler type.
        // This runs during single-threaded IDT initialization.
        unsafe {
            let entry_ptr = timer_interrupt_entry as *const ();
            idt[InterruptIndex::Timer.as_u8()].set_handler_fn(core::mem::transmute::<
                *const (),
                extern "x86-interrupt" fn(InterruptStackFrame),
            >(entry_ptr));

            let yield_entry_ptr = yield_interrupt_entry as *const ();
            idt[YIELD_INTERRUPT_VECTOR].set_handler_fn(core::mem::transmute::<
                *const (),
                extern "x86-interrupt" fn(InterruptStackFrame),
            >(yield_entry_ptr));
        }

        idt[KEYBOARD_INTERRUPT_VECTOR].set_handler_fn(keyboard_interrupt_handler);
        idt[SCI_INTERRUPT_VECTOR].set_handler_fn(sci_interrupt_handler);
        for vector in ACPI_PCI_VECTOR_BASE..ACPI_PCI_VECTOR_BASE + ACPI_PCI_VECTOR_COUNT {
            idt[vector].set_handler_fn(generic_external_interrupt_handler);
        }
        idt[SYSCALL_VECTOR].set_handler_fn(syscall_handler);
        idt
    };
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use crate::input::KEYBOARD;
    use pc_keyboard::{DecodedKey, KeyState};
    use turnix_abi::input::{InputEvent, KEY_STATE_PRESSED, KEY_STATE_RELEASED};
    use x86_64::instructions::port::Port;

    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);

    // SAFETY: Port 0x60 is the PS/2 keyboard data port, a valid I/O port.
    // Reading from it returns the current scancode from the keyboard controller.
    let scancode: u8 = unsafe { port.read() };
    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        // Push the raw key event (press/release) for the normalized event bus.
        let pressed = key_event.state == KeyState::Down;
        let value = if pressed {
            KEY_STATE_PRESSED
        } else {
            KEY_STATE_RELEASED
        };
        let keycode = crate::input::ps2_keycode_to_key(key_event.code);
        crate::input::add_event(InputEvent::new(
            turnix_abi::input::INPUT_KIND_KEY,
            keycode,
            value,
        ));

        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(character) => {
                    crate::tty::TTY.lock().handle_input(character);
                    crate::input::add_char(character);
                }
                DecodedKey::RawKey(_) => {}
            }
        }
    }

    // SAFETY: Direct LAPIC EOI via LAPIC_BASE set during init().
    unsafe {
        lapic_write(0xB0, 0);
    }
}

extern "x86-interrupt" fn sci_interrupt_handler(_stack_frame: InterruptStackFrame) {
    crate::acpi::handle_sci_interrupt();
    // SAFETY: Direct LAPIC EOI via LAPIC_BASE set during init().
    unsafe {
        lapic_write(0xB0, 0);
    }
}

extern "x86-interrupt" fn generic_external_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // SAFETY: Direct LAPIC EOI via LAPIC_BASE set during init().
    unsafe {
        lapic_write(0xB0, 0);
    }
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    crate::serial::print(format_args!("EXCEPTION: BREAKPOINT\n{:#?}\n", stack_frame));
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    use x86_64::registers::control::Cr2;
    let cr2 = Cr2::read_raw();
    crate::serial::println!("EXCEPTION: DOUBLE FAULT");
    crate::serial::println!(
        "  RIP={:#018x} CS={:#06x}",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.code_segment.0 as u64,
    );
    crate::serial::println!(
        "  RSP={:#018x} SS={:#06x} CR2={:#018x}",
        stack_frame.stack_pointer.as_u64(),
        stack_frame.stack_segment.0 as u64,
        cr2,
    );
    // Dump stack near RSP
    let rsp = stack_frame.stack_pointer.as_u64();
    // SAFETY: We are in a double fault handler (unrecoverable). rsp is from
    // the interrupt stack frame and points to valid kernel memory. The 16
    // reads cover the immediate stack vicinity for diagnostic purposes.
    unsafe {
        for i in 0..16u64 {
            let addr = rsp + i * 8;
            let ptr = addr as *const u64;
            crate::serial::println!("  [{:#018x}] = {:#018x}", addr, ptr.read_volatile());
        }
    }
    panic!("DOUBLE FAULT");
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    use x86_64::registers::model_specific::{GsBase, KernelGsBase};

    let addr = Cr2::read().unwrap_or(VirtAddr::new(0xFFFF_FFFF_FFFF_FFFF));

    // Attempt demand paging for user-mode faults
    if stack_frame.code_segment.0 & 0x3 == 0x3 {
        // ── DIAGNOSTIC: always log user-mode page faults ─────
        crate::serial::println!(
            "[PF_USER] addr={:#x} err={:?} rip={:#x}",
            addr.as_u64(), error_code, stack_frame.instruction_pointer.as_u64(),
        );
        if crate::memory::demand::handle_demand_fault() {
            crate::serial::println!("[PF_USER] demand fault HANDLED for {:#x}", addr.as_u64());
            return;
        }
        crate::serial::println!(
            "PROCESS FAULT: Page Fault at {:?} with error code {:?}. Terminating process.",
            addr,
            error_code
        );
        crate::task::scheduler::exit_current_task();
    }

    crate::serial::println!("[STG: PF_KERNEL addr={:?} err={:?} cs={:#x} rip={:?}]", addr, error_code, stack_frame.code_segment.0, stack_frame.instruction_pointer);

    {
        use x86_64::registers::control::Cr3;
        let (cr3_val, _) = Cr3::read();
        let pml4_phys = cr3_val.start_address();
        crate::serial::println!(
            "[PF] CR3={:#x} RSP=0x{:x}",
            pml4_phys.as_u64(),
            stack_frame.stack_pointer.as_u64(),
        );
        // Dump current instruction bytes at RIP if we can read them safely
        let rip = stack_frame.instruction_pointer;
        crate::serial::print(format_args!("[PF] RIP bytes:"));
        for i in 0..16 {
            let byte_ptr = (rip.as_u64() + i) as *const u8;
            // SAFETY: We are in a page fault handler on the panic path.
            // read_volatile prevents the compiler from optimizing away the
            // diagnostic read. If the address is unmapped, the serial output
            // before this point will have been printed.
            unsafe {
                let val = core::ptr::read_volatile(byte_ptr);
                crate::serial::print(format_args!(" {:02x}", val));
            }
        }
        crate::serial::println!("");
    }

    let gs_base = GsBase::read();
    let kernel_gs_base = KernelGsBase::read();

    panic!(
        "EXCEPTION: PAGE FAULT in Kernel\nAccessed Address: {:?}\nError Code: {:?}\nGS_BASE: {:?}, KERNEL_GS_BASE: {:?}\n{:#?}",
        addr, error_code, gs_base, kernel_gs_base, stack_frame
    );
}

extern "x86-interrupt" fn gpf_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    use x86_64::registers::model_specific::{GsBase, KernelGsBase};

    if stack_frame.code_segment.0 & 0x3 == 0x3 {
        crate::serial::println!(
            "PROCESS FAULT: General Protection Fault with error code {}. Terminating process.",
            error_code
        );
        crate::serial::println!(
            "  CS={:#x} RIP={:?} RSP={:?}",
            stack_frame.code_segment.0,
            stack_frame.instruction_pointer,
            stack_frame.stack_pointer
        );
        crate::serial::println!("  RFLAGS={:#x}", stack_frame.cpu_flags);
        crate::task::scheduler::exit_current_task();
    }

    let gs_base = GsBase::read();
    let kernel_gs_base = KernelGsBase::read();

    crate::serial::println!("EXCEPTION: GENERAL PROTECTION FAULT in Kernel");
    crate::serial::println!("Error Code: {:#x}", error_code);
    crate::serial::println!(
        "GS_BASE: {:?}, KERNEL_GS_BASE: {:?}",
        gs_base,
        kernel_gs_base
    );
    crate::serial::println!("Instruction Pointer: {:?}", stack_frame.instruction_pointer);
    crate::serial::println!("Stack Pointer: {:?}", stack_frame.stack_pointer);
    crate::serial::println!("Code Segment: {:#x}", stack_frame.code_segment.0);
    crate::serial::println!("RFLAGS: {:#x}", stack_frame.cpu_flags);
    panic!("GPF");
}

extern "x86-interrupt" fn syscall_handler(_stack_frame: InterruptStackFrame) {
    crate::serial::print(format_args!("[syscall int 0x80]\n"));
}

pub fn configure_ioapic_route(
    ioapic_physical_address: u32,
    input: u8,
    vector: u8,
    active_low: bool,
    level_triggered: bool,
) -> Result<(), &'static str> {
    let phys_mem_offset = crate::boot::get_phys_mem_offset();
    let ioapic_virt = phys_mem_offset + ioapic_physical_address as u64;
    // SAFETY: ioapic_virt is computed from a valid physical MMIO address plus
    // the physical memory offset. The address is within the standard IOAPIC
    // MMIO region.
    let mut ioapic = unsafe { apic::IoApic::new(ioapic_virt) };
    // SAFETY: The IOAPIC was just constructed with a valid virtual address.
    // We are the only thread accessing it (called during setup or with
    // appropriate external synchronization).
    unsafe {
        ioapic.route_irq_configured(input, vector, active_low, level_triggered);
    }
    Ok(())
}
