use core::sync::atomic::{AtomicU32, Ordering};
use x86_64::VirtAddr;

pub const MAX_CPUS: usize = 256;
pub const TRAMPOLINE_ADDR: u64 = 0x8000;

#[repr(align(4096))]
#[derive(Clone, Copy)]
pub struct PerCpuData {
    pub cpu_id: u32,
    pub kernel_stack_top: u64,
    pub current_task_id: u64,
    pub uptime_ticks: u64,
    pub is_bsp: bool,
}

impl Default for PerCpuData {
    fn default() -> Self {
        Self::new()
    }
}

impl PerCpuData {
    pub const fn new() -> Self {
        Self {
            cpu_id: 0,
            kernel_stack_top: 0,
            current_task_id: 0,
            uptime_ticks: 0,
            is_bsp: true,
        }
    }
}

static mut PER_CPU_AREA: [PerCpuData; MAX_CPUS] = [PerCpuData::new(); MAX_CPUS];
static CPU_COUNT: AtomicU32 = AtomicU32::new(1);
static BSP_CPU_ID: AtomicU32 = AtomicU32::new(0);
static AP_READY: AtomicU32 = AtomicU32::new(0);

pub fn get_cpu_count() -> u32 {
    CPU_COUNT.load(Ordering::Relaxed)
}

pub fn get_bsp_cpu_id() -> u32 {
    BSP_CPU_ID.load(Ordering::Relaxed)
}

pub fn get_current_cpu_id() -> u32 {
    match crate::acpi::get_lapic_address() {
        Some(lapic_base) => {
            // SAFETY: `lapic_base` is a valid LAPIC MMIO physical address obtained
            // from ACPI; the +0x20 offset is the LAPIC ID register, which is
            // always 4-byte aligned and readable on x86_64 APIC-capable hardware.
            let lapic_id_reg =
                unsafe { core::ptr::read_volatile((lapic_base as usize + 0x20) as *const u32) };
            lapic_id_reg >> 24
        }
        None => 0,
    }
}

pub fn set_current_cpu_data(data: &PerCpuData) {
    let idx = data.cpu_id as usize;
    if idx < MAX_CPUS {
        // SAFETY: `idx` is bounds-checked to be less than MAX_CPUS immediately
        // above, so the slice access is valid. `PER_CPU_AREA` is a static
        // mut array; each CPU only writes its own index, preventing data races
        // (callers must ensure CPU-local exclusivity).
        unsafe {
            PER_CPU_AREA[idx] = *data;
        }
    }
}

pub fn get_cpu_data(cpu_id: u32) -> &'static PerCpuData {
    let idx = cpu_id as usize;
    if idx < MAX_CPUS {
        // SAFETY: `idx` is less than MAX_CPUS (checked above), so the array
        // access is in-bounds. The returned reference has 'static lifetime
        // because PER_CPU_AREA is a static. The caller must not hold the
        // reference across a write to the same slot.
        unsafe { &PER_CPU_AREA[idx] }
    } else {
        // SAFETY: Slot 0 always exists (MAX_CPUS >= 1). Used as a safe
        // fallback for out-of-range CPU IDs.
        unsafe { &PER_CPU_AREA[0] }
    }
}

pub fn get_current_cpu_data() -> &'static PerCpuData {
    let cpu_id = get_current_cpu_id();
    get_cpu_data(cpu_id)
}

pub fn increment_cpu_count() {
    CPU_COUNT.fetch_add(1, Ordering::SeqCst);
}

pub fn set_bsp_cpu_id(id: u32) {
    BSP_CPU_ID.store(id, Ordering::SeqCst);
}

pub fn signal_ap_ready() {
    AP_READY.fetch_add(1, Ordering::SeqCst);
}

pub fn wait_for_aps(timeout_us: u64) -> u32 {
    // SAFETY: `_rdtsc` is a read-only, non-privileged hardware instruction.
    // It has no side effects and is always safe to call on x86_64.
    let start = unsafe { core::arch::x86_64::_rdtsc() };
    loop {
        let ready = AP_READY.load(Ordering::Relaxed);
        if ready >= get_cpu_count() - 1 {
            return ready;
        }
        // SAFETY: Same as above — `_rdtsc` is a non-privileged read-only
        // instruction with no memory side effects.
        let now = unsafe { core::arch::x86_64::_rdtsc() };
        if now - start > timeout_us * 1000 {
            return ready;
        }
        core::hint::spin_loop();
    }
}

pub fn init(_phys_mem_offset: VirtAddr) {
    crate::serial::println!("[SMP] Initializing...");

    if let Some(bsp_id) = crate::acpi::get_boot_apic_id() {
        set_bsp_cpu_id(bsp_id);
        crate::serial::println!("[SMP] BSP CPU ID: {}", bsp_id);
    } else {
        crate::serial::println!("[SMP] No BSP ID from ACPI - assuming CPU 0");
        set_bsp_cpu_id(0);
    }

    let ap_ids = crate::acpi::get_ap_apic_ids();
    let ap_count = ap_ids.len();
    crate::serial::println!("[SMP] Found {} Application Processors", ap_count);

    if ap_count > 0 {
        crate::serial::println!("[SMP] Starting AP boot sequence...");
        start_aps(&ap_ids);
    }

    crate::serial::println!("[SMP] Running with {} CPU(s)", get_cpu_count());
}

fn start_aps(ap_ids: &[u32]) {
    use x86_64::instructions::interrupts;

    interrupts::without_interrupts(|| {
        for &apic_id in ap_ids {
            crate::serial::println!("[SMP] Starting AP with LAPIC ID {}", apic_id);
            if !send_init_sipi_sipi(apic_id) {
                crate::serial::println!("[SMP] Failed to start AP {}", apic_id);
            }
        }
    });

    let ready = wait_for_aps(1000000);
    crate::serial::println!("[SMP] {} APs started successfully", ready);
}

fn send_init_sipi_sipi(apic_id: u32) -> bool {
    let lapic_base = match crate::acpi::get_lapic_address() {
        Some(addr) => addr as usize,
        None => return false,
    };

    let icr_low = lapic_base + 0x300;
    let icr_high = lapic_base + 0x310;

    // SAFETY: `icr_low` and `icr_high` are MMIO register addresses derived
    // from the LAPIC base (validated by ACPI) at the standard ICR offsets
    // (0x300 and 0x310). Volatile writes are used to prevent the compiler
    // from reordering or eliding the MMIO register accesses. All accesses are
    // 4-byte aligned. No Rust references overlap these raw addresses.
    unsafe {
        core::ptr::write_volatile(icr_high as *mut u32, apic_id << 24);
        core::ptr::write_volatile(icr_low as *mut u32, 0x0000C500);

        while core::ptr::read_volatile(icr_low as *const u32) & (1 << 12) != 0 {
            core::hint::spin_loop();
        }

        for _ in 0..1000000 {
            core::hint::spin_loop();
        }

        let trampoline_vec = (TRAMPOLINE_ADDR >> 12) as u32;
        core::ptr::write_volatile(icr_high as *mut u32, apic_id << 24);
        core::ptr::write_volatile(icr_low as *mut u32, 0x0000C600 | (trampoline_vec & 0xFF));

        while core::ptr::read_volatile(icr_low as *const u32) & (1 << 12) != 0 {
            core::hint::spin_loop();
        }

        for _ in 0..20000 {
            core::hint::spin_loop();
        }

        core::ptr::write_volatile(icr_high as *mut u32, apic_id << 24);
        core::ptr::write_volatile(icr_low as *mut u32, 0x0000C600 | (trampoline_vec & 0xFF));

        while core::ptr::read_volatile(icr_low as *const u32) & (1 << 12) != 0 {
            core::hint::spin_loop();
        }
    }

    true
}

#[unsafe(no_mangle)]
pub extern "C" fn ap_entry(apic_id: u32) -> ! {
    crate::serial::println!("[AP {}] Started", apic_id);

    let mut cpu_data = PerCpuData::new();
    cpu_data.cpu_id = apic_id;
    cpu_data.is_bsp = false;
    set_current_cpu_data(&cpu_data);

    crate::gdt::init_for_cpu(apic_id);
    crate::interrupts::apic::init_for_cpu();

    signal_ap_ready();

    crate::serial::println!("[AP {}] Entering idle loop", apic_id);
    loop {
        x86_64::instructions::hlt();
    }
}
