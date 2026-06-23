use x86_64::VirtAddr;
use x86_64::registers::model_specific::Msr;

/// The Local APIC (LAPIC) is responsible for handling interrupts for a single CPU.
pub struct LocalApic {
    base_addr: VirtAddr,
}

impl LocalApic {
    /// Create a new Local APIC instance.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the base address is valid and mapped.
    // SAFETY: Caller must ensure base_addr is a valid, mapped LAPIC MMIO address.
    pub unsafe fn new(base_addr: VirtAddr) -> Self {
        Self { base_addr }
    }

    /// Read a register from the Local APIC.
    // SAFETY: Caller of `read` guarantees self.base_addr points to a valid LAPIC MMIO region.
    unsafe fn read(&self, offset: u32) -> u32 {
        unsafe {
            let ptr = (self.base_addr.as_u64() + offset as u64) as *const u32;
            ptr.read_volatile()
        }
    }

    /// Write a register to the Local APIC.
    // SAFETY: Caller of `write` guarantees self.base_addr points to a valid LAPIC MMIO region.
    unsafe fn write(&mut self, offset: u32, value: u32) {
        unsafe {
            let ptr = (self.base_addr.as_u64() + offset as u64) as *mut u32;
            ptr.write_volatile(value);
        }
    }

    /// Initialize the Local APIC.
    ///
    /// # Safety
    ///
    /// The LAPIC MMIO region must be mapped and valid.
    pub unsafe fn initialize(&mut self) {
        // Enable the Local APIC by setting bit 8 of the Spurious Interrupt Vector Register.
        // We also set the spurious vector to 0xFF.
        // SAFETY: The caller guarantees the LAPIC MMIO region is mapped and valid.
        unsafe {
            let spurious_vector = 0xFF;
            self.write(0xF0, self.read(0xF0) | 0x100 | spurious_vector);
        }
    }

    /// Set the APIC timer to fire every `count` ticks.
    ///
    /// # Safety
    ///
    /// The LAPIC MMIO region must be mapped and valid.
    pub unsafe fn start_timer(&mut self, count: u32) {
        // SAFETY: The caller guarantees the LAPIC MMIO region is mapped and valid.
        unsafe {
            // Divide by 16
            self.write(0x3E0, 0x3);
            // Periodic mode, vector 32
            self.write(0x320, 0x20000 | 32);
            // Initial count
            self.write(0x380, count);
        }
    }

    /// Signal End of Interrupt (EOI) to the Local APIC.
    ///
    /// # Safety
    ///
    /// The LAPIC MMIO region must be mapped and valid.
    pub unsafe fn signal_eoi(&mut self) {
        // SAFETY: The caller guarantees the LAPIC MMIO region is mapped and valid.
        unsafe {
            self.write(0xB0, 0);
        }
    }
}

pub struct IoApic {
    base_addr: VirtAddr,
}

impl IoApic {
    /// # Safety
    ///
    /// The caller must ensure that the base address is valid and mapped.
    // SAFETY: Caller must ensure base_addr is a valid, mapped IOAPIC MMIO address.
    pub unsafe fn new(base_addr: VirtAddr) -> Self {
        Self { base_addr }
    }

    // SAFETY: Caller of `write` guarantees self.base_addr points to a valid IOAPIC MMIO region.
    unsafe fn write(&mut self, reg: u32, value: u32) {
        unsafe {
            let ioapic_ptr = self.base_addr.as_u64() as *mut u32;
            ioapic_ptr.write_volatile(reg);
            ioapic_ptr.add(4).write_volatile(value);
        }
    }

    /// # Safety
    ///
    /// The IOAPIC MMIO region must be mapped and valid.
    pub unsafe fn route_irq(&mut self, irq: u8, vector: u8) {
        // SAFETY: The caller guarantees the IOAPIC MMIO region is mapped and valid.
        unsafe { self.route_irq_configured(irq, vector, false, false) };
    }

    /// # Safety
    ///
    /// The IOAPIC MMIO region must be mapped and valid.
    pub unsafe fn route_irq_configured(
        &mut self,
        irq: u8,
        vector: u8,
        active_low: bool,
        level_triggered: bool,
    ) {
        let low_reg = 0x10 + (irq as u32) * 2;
        let high_reg = low_reg + 1;
        let mut low = vector as u32;

        if active_low {
            low |= 1 << 13;
        }
        if level_triggered {
            low |= 1 << 15;
        }

        // SAFETY: The caller guarantees the IOAPIC MMIO region is mapped and valid, and irq is in range.
        unsafe {
            self.write(low_reg, low);
            self.write(high_reg, 0);
        }
    }
}

/// Get the physical base address of the Local APIC from the IA32_APIC_BASE MSR.
pub fn get_base_addr() -> VirtAddr {
    let mut apic_base_msr = Msr::new(0x1B);
    // SAFETY: MSR access is safe on x86_64; IA32_APIC_BASE (0x1B) is a standard architectural MSR.
    unsafe {
        let base = apic_base_msr.read();
        // Set bit 11 (APIC Global Enable) if not already set
        if (base & 0x800) == 0 {
            apic_base_msr.write(base | 0x800);
        }
        // The base address is in bits 12-51 (for x86_64)
        let addr = base & 0xFFFFFFFFFF000;
        VirtAddr::new(addr)
    }
}

pub fn init_for_cpu() {
    let base = get_base_addr();
    // SAFETY: base was read from IA32_APIC_BASE MSR and is a valid LAPIC address.
    let mut lapic = unsafe { LocalApic::new(base) };
    // SAFETY: The LAPIC MMIO region is mapped and valid as per get_base_addr().
    unsafe {
        lapic.initialize();
        lapic.start_timer(0x10000);
    }
}
