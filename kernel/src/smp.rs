use core::sync::atomic::{AtomicU32, Ordering};
use x86_64::VirtAddr;

pub const MAX_CPUS: usize = 256;

#[repr(align(4096))]
#[derive(Clone, Copy)]
pub struct PerCpuData {
    pub cpu_id: u32,
    pub kernel_stack_top: u64,
    pub current_task_id: u64,
    pub uptime_ticks: u64,
    pub is_bsp: bool,
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

pub fn get_cpu_count() -> u32 {
    CPU_COUNT.load(Ordering::Relaxed)
}

pub fn get_bsp_cpu_id() -> u32 {
    BSP_CPU_ID.load(Ordering::Relaxed)
}

pub fn get_current_cpu_id() -> u32 {
    match crate::acpi::get_lapic_address() {
        Some(lapic_base) => {
            let lapic_id_reg = unsafe {
                core::ptr::read_volatile((lapic_base as usize + 0x20) as *const u32)
            };
            lapic_id_reg >> 24
        }
        None => 0, // BSP default
    }
}

pub fn set_current_cpu_data(data: &PerCpuData) {
    let idx = data.cpu_id as usize;
    if idx < MAX_CPUS {
        unsafe {
            PER_CPU_AREA[idx] = data.clone();
        }
    }
}

pub fn get_cpu_data(cpu_id: u32) -> &'static PerCpuData {
    let idx = cpu_id as usize;
    if idx < MAX_CPUS {
        unsafe { &PER_CPU_AREA[idx] }
    } else {
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

pub fn init(_phys_mem_offset: VirtAddr) {
    crate::serial::println!("[SMP] Initializing...");

    // Get BSP ID from ACPI
    if let Some(bsp_id) = crate::acpi::get_boot_apic_id() {
        set_bsp_cpu_id(bsp_id);
        crate::serial::println!("[SMP] BSP CPU ID: {}", bsp_id);
    } else {
        crate::serial::println!("[SMP] No BSP ID from ACPI - assuming CPU 0");
        set_bsp_cpu_id(0);
    }

    // Get AP IDs
    let ap_ids = crate::acpi::get_ap_apic_ids();
    let ap_count = ap_ids.len();
    crate::serial::println!("[SMP] Found {} Application Processors", ap_count);

    if ap_count > 0 {
        crate::serial::println!("[SMP] AP boot not yet implemented - running UP kernel");
    }

    crate::serial::println!("[SMP] Running with {} CPU(s)", get_cpu_count());
}