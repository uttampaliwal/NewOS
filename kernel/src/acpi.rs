use acpi::{AcpiHandler, AcpiTables, PhysicalMapping};
use core::ptr::NonNull;
use x86_64::VirtAddr;

#[derive(Clone)]
pub struct TurnixAcpiHandler {
    phys_mem_offset: VirtAddr,
}

impl TurnixAcpiHandler {
    pub fn new(phys_mem_offset: VirtAddr) -> Self {
        Self { phys_mem_offset }
    }
}

impl AcpiHandler for TurnixAcpiHandler {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        let virtual_address = self.phys_mem_offset + physical_address as u64;
        unsafe {
            PhysicalMapping::new(
                physical_address,
                NonNull::new(virtual_address.as_mut_ptr()).unwrap(),
                size,
                size,
                self.clone(),
            )
        }
    }

    fn unmap_physical_region<T>(_region: &PhysicalMapping<Self, T>) {
        // We use a fixed physical memory offset, so we don't need to unmap anything.
    }
}

pub fn init(rsdp_addr: u64, phys_mem_offset: VirtAddr) {
    if rsdp_addr == 0 {
        crate::serial::println!("[ACPI] No RSDP provided by bootloader.");
        return;
    }

    crate::serial::println!("[ACPI] Initializing... RSDP at {:#x}", rsdp_addr);

    let handler = TurnixAcpiHandler::new(phys_mem_offset);
    let acpi_tables = unsafe { AcpiTables::from_rsdp(handler, rsdp_addr as usize) };

    match acpi_tables {
        Ok(tables) => {
            crate::serial::println!("[ACPI] Tables parsed successfully.");
            
            if let Ok(platform_info) = acpi::PlatformInfo::new(&tables) {
                crate::serial::println!("[ACPI] Platform Info parsed.");
                
                if let acpi::InterruptModel::Apic(apic_info) = platform_info.interrupt_model {
                    crate::serial::println!("[ACPI] APIC Model detected.");
                    crate::serial::println!("[ACPI] Local APIC address: {:#x}", apic_info.local_apic_address);
                }
                
                if let Some(processor_info) = platform_info.processor_info {
                    crate::serial::println!(
                        "[ACPI] Boot Processor: APIC ID {}, State: {:?}",
                        processor_info.boot_processor.local_apic_id, processor_info.boot_processor.state
                    );
                    
                    let mut ap_count = 0;
                    for proc in processor_info.application_processors.iter() {
                        crate::serial::println!(
                            "[ACPI] Found Application Processor (LAPIC ID: {}, State: {:?})",
                            proc.local_apic_id, proc.state
                        );
                        ap_count += 1;
                    }
                    crate::serial::println!("[ACPI] Total Application Processors: {}", ap_count);
                } else {
                    crate::serial::println!("[ACPI] No processor info found.");
                }
            }
        }
        Err(e) => {
            crate::serial::println!("[ACPI] Failed to parse tables: {:?}", e);
        }
    }
}
