use acpi::{AcpiHandler, AcpiTables, PhysicalMapping};
use alloc::boxed::Box;
use aml::{AmlContext, AmlName, DebugVerbosity, Handler};
use aml::value::Args;
use core::ptr::NonNull;
use spin::Mutex;
use x86_64::VirtAddr;

static LAPIC_ADDRESS: Mutex<Option<u64>> = Mutex::new(None);
static BOOT_APIC_ID: Mutex<Option<u32>> = Mutex::new(None);
static AP_APIC_IDS: Mutex<alloc::vec::Vec<u32>> = Mutex::new(alloc::vec::Vec::new());
static PCI_ROUTING_TABLE: Mutex<alloc::vec::Vec<PciRoutingEntry>> = Mutex::new(alloc::vec::Vec::new());

#[derive(Debug, Clone)]
pub struct PciRoutingEntry {
    pub address: u32, // PCI address (bus, device, function)
    pub pin: u8,      // Interrupt pin (0-3)
    pub source: Option<AmlName>, // Source device name or None for GSI
    pub source_index: u8, // Source index
}

pub fn get_pci_routing_table() -> alloc::vec::Vec<PciRoutingEntry> {
    PCI_ROUTING_TABLE.lock().clone()
}

pub fn get_lapic_address() -> Option<u64> {
    *LAPIC_ADDRESS.lock()
}

pub fn get_boot_apic_id() -> Option<u32> {
    *BOOT_APIC_ID.lock()
}

pub fn get_ap_apic_ids() -> alloc::vec::Vec<u32> {
    AP_APIC_IDS.lock().clone()
}

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

struct AmlAcpiHandler {
    phys_mem_offset: VirtAddr,
}

impl AmlAcpiHandler {
    fn new(phys_mem_offset: VirtAddr) -> Self {
        Self { phys_mem_offset }
    }

    fn translate_address(&self, physical_address: usize) -> usize {
        (self.phys_mem_offset + physical_address as u64).as_u64() as usize
    }
}

impl Handler for AmlAcpiHandler {
    fn read_u8(&self, address: usize) -> u8 {
        let ptr = self.translate_address(address) as *const u8;
        unsafe { core::ptr::read_volatile(ptr) }
    }

    fn read_u16(&self, address: usize) -> u16 {
        let ptr = self.translate_address(address) as *const u16;
        unsafe { core::ptr::read_volatile(ptr) }
    }

    fn read_u32(&self, address: usize) -> u32 {
        let ptr = self.translate_address(address) as *const u32;
        unsafe { core::ptr::read_volatile(ptr) }
    }

    fn read_u64(&self, address: usize) -> u64 {
        let ptr = self.translate_address(address) as *const u64;
        unsafe { core::ptr::read_volatile(ptr) }
    }

    fn write_u8(&mut self, _address: usize, _value: u8) {}
    fn write_u16(&mut self, _address: usize, _value: u16) {}
    fn write_u32(&mut self, _address: usize, _value: u32) {}
    fn write_u64(&mut self, _address: usize, _value: u64) {}

    fn read_io_u8(&self, _port: u16) -> u8 {
        0
    }

    fn read_io_u16(&self, _port: u16) -> u16 {
        0
    }

    fn read_io_u32(&self, _port: u16) -> u32 {
        0
    }

    fn write_io_u8(&self, _port: u16, _value: u8) {}
    fn write_io_u16(&self, _port: u16, _value: u16) {}
    fn write_io_u32(&self, _port: u16, _value: u32) {}

    fn read_pci_u8(
        &self,
        _segment: u16,
        _bus: u8,
        _device: u8,
        _function: u8,
        _offset: u16,
    ) -> u8 {
        0
    }

    fn read_pci_u16(
        &self,
        _segment: u16,
        _bus: u8,
        _device: u8,
        _function: u8,
        _offset: u16,
    ) -> u16 {
        0
    }

    fn read_pci_u32(
        &self,
        _segment: u16,
        _bus: u8,
        _device: u8,
        _function: u8,
        _offset: u16,
    ) -> u32 {
        0
    }

    fn write_pci_u8(
        &self,
        _segment: u16,
        _bus: u8,
        _device: u8,
        _function: u8,
        _offset: u16,
        _value: u8,
    ) {
    }

    fn write_pci_u16(
        &self,
        _segment: u16,
        _bus: u8,
        _device: u8,
        _function: u8,
        _offset: u16,
        _value: u16,
    ) {
    }

    fn write_pci_u32(
        &self,
        _segment: u16,
        _bus: u8,
        _device: u8,
        _function: u8,
        _offset: u16,
        _value: u32,
    ) {
    }
}

fn parse_aml_table<H>(handler: &H, table: &acpi::AmlTable, context: &mut AmlContext) -> Result<(), &'static str>
where
    H: AcpiHandler,
{
    let mapping = unsafe { handler.map_physical_region::<u8>(table.address, table.length as usize) };
    let data = unsafe { core::slice::from_raw_parts(mapping.virtual_start().as_ptr(), table.length as usize) };
    let mut aml_bytes = alloc::vec![0u8; data.len()];
    aml_bytes.copy_from_slice(data);

    context
        .parse_table(&aml_bytes)
        .map_err(|_| "failed to parse AML table")?;

    Ok(())
}

fn parse_acpi_aml_tables<H>(phys_mem_offset: VirtAddr, handler: &H, tables: &AcpiTables<H>) -> Result<AmlContext, &'static str>
where
    H: AcpiHandler,
{
    let mut context = AmlContext::new(
        Box::new(AmlAcpiHandler::new(phys_mem_offset)),
        DebugVerbosity::None,
    );

    if let Some(ref dsdt) = tables.dsdt {
        parse_aml_table(handler, dsdt, &mut context)?;
    }

    for ssdt in tables.ssdts.iter() {
        parse_aml_table(handler, ssdt, &mut context)?;
    }

    Ok(context)
}

fn parse_prt_result(result: aml::value::AmlValue) -> Result<(), &'static str> {
    use aml::value::AmlValue;

    let mut routing_table = PCI_ROUTING_TABLE.lock();

    match result {
        AmlValue::Package(entries) => {
            for entry in entries {
                match entry {
                    AmlValue::Package(fields) if fields.len() >= 4 => {
                        // _PRT entry format: [Address, Pin, Source, SourceIndex]
                        let address = match &fields[0] {
                            AmlValue::Integer(addr) => *addr as u32,
                            _ => continue,
                        };
                        let pin = match &fields[1] {
                            AmlValue::Integer(p) => *p as u8,
                            _ => continue,
                        };
                        let source = match &fields[2] {
                            AmlValue::String(s) => Some(AmlName::from_str(s).map_err(|_| "invalid source name")?),
                            _ => None, // GSI case
                        };
                        let source_index = match &fields[3] {
                            AmlValue::Integer(idx) => *idx as u8,
                            _ => continue,
                        };

                        routing_table.push(PciRoutingEntry {
                            address,
                            pin,
                            source,
                            source_index,
                        });
                    }
                    _ => continue,
                }
            }
            crate::serial::println!("[ACPI] Parsed {} PCI routing entries.", routing_table.len());
            Ok(())
        }
        _ => Err("Invalid _PRT result format"),
    }
}

fn evaluate_sb_namespace(context: &mut AmlContext) {
    let sb_name = AmlName::from_str("\\_SB").unwrap();
    let prt_name = AmlName::from_str("\\_SB._PRT").unwrap();

    if context.namespace.get_by_path(&sb_name).is_ok() {
        crate::serial::println!("[ACPI] ACPI namespace _SB found and parsed.");

        match context.invoke_method(&prt_name, Args::default()) {
            Ok(result) => {
                crate::serial::println!("[ACPI] _PRT evaluation succeeded.");
                if let Err(e) = parse_prt_result(result) {
                    crate::serial::println!("[ACPI] Failed to parse _PRT result: {:?}", e);
                }
            }
            Err(e) => {
                crate::serial::println!("[ACPI] Failed to evaluate _PRT: {:?}", e);
            }
        }
    } else {
        crate::serial::println!("[ACPI] ACPI namespace _SB not found.");
    }
}

pub fn init(rsdp_addr: u64, phys_mem_offset: VirtAddr) {
    if rsdp_addr == 0 {
        crate::serial::println!("[ACPI] No RSDP provided by bootloader.");
        return;
    }

    crate::serial::println!("[ACPI] Initializing... RSDP at {:#x}", rsdp_addr);

    let handler = TurnixAcpiHandler::new(phys_mem_offset);
    let acpi_tables = unsafe { AcpiTables::from_rsdp(handler.clone(), rsdp_addr as usize) };

    match acpi_tables {
        Ok(tables) => {
            crate::serial::println!("[ACPI] Tables parsed successfully.");
            
            if let Ok(platform_info) = acpi::PlatformInfo::new(&tables) {
                crate::serial::println!("[ACPI] Platform Info parsed.");
                
                if let acpi::InterruptModel::Apic(apic_info) = platform_info.interrupt_model {
                    crate::serial::println!("[ACPI] APIC Model detected.");
                    crate::serial::println!("[ACPI] Local APIC address: {:#x}", apic_info.local_apic_address);
                    *LAPIC_ADDRESS.lock() = Some(apic_info.local_apic_address);
                }
                
                if let Some(processor_info) = platform_info.processor_info {
                    *BOOT_APIC_ID.lock() = Some(processor_info.boot_processor.local_apic_id);
                    
                    crate::serial::println!(
                        "[ACPI] Boot Processor: APIC ID {}, State: {:?}",
                        processor_info.boot_processor.local_apic_id, processor_info.boot_processor.state
                    );
                    
                    let mut ap_count = 0;
                    let mut ap_ids = AP_APIC_IDS.lock();
                    for proc in processor_info.application_processors.iter() {
                        crate::serial::println!(
                            "[ACPI] Found Application Processor (LAPIC ID: {}, State: {:?})",
                            proc.local_apic_id, proc.state
                        );
                        ap_ids.push(proc.local_apic_id);
                        ap_count += 1;
                    }
                    crate::serial::println!("[ACPI] Total Application Processors: {}", ap_count);
                } else {
                    crate::serial::println!("[ACPI] No processor info found.");
                }
            }

            match parse_acpi_aml_tables(phys_mem_offset, &handler, &tables) {
                Ok(mut context) => {
                    crate::serial::println!("[ACPI] DSDT and SSDTs parsed into AML context.");
                    evaluate_sb_namespace(&mut context);
                }
                Err(e) => {
                    crate::serial::println!("[ACPI] Failed to parse AML tables: {}", e);
                }
            }
        }
        Err(e) => {
            crate::serial::println!("[ACPI] Failed to parse tables: {:?}", e);
        }
    }
}
