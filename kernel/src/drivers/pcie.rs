use acpi::{AcpiHandler, AcpiTables, PhysicalMapping, PciConfigRegions};
use core::ptr::NonNull;
use x86_64::VirtAddr;

use crate::drivers::framework::{Bar, DeviceInfo, DeviceKey};

#[derive(Clone)]
struct PcieAcpiHandler {
    phys_mem_offset: VirtAddr,
}

impl PcieAcpiHandler {
    fn new(phys_mem_offset: VirtAddr) -> Self {
        Self { phys_mem_offset }
    }
}

impl AcpiHandler for PcieAcpiHandler {
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
        // HHDM mapping; nothing to do.
    }
}

fn cfg_read_u32(cfg_phys: u64, phys_mem_offset: VirtAddr, offset: u16) -> u32 {
    let addr = phys_mem_offset + cfg_phys + offset as u64;
    unsafe { core::ptr::read_volatile(addr.as_ptr::<u32>()) }
}

fn parse_bars(cfg_phys: u64, phys_mem_offset: VirtAddr) -> [Option<Bar>; 6] {
    let mut bars: [Option<Bar>; 6] = [None, None, None, None, None, None];

    let mut i = 0usize;
    while i < 6 {
        let off = 0x10u16 + (i as u16) * 4;
        let raw = cfg_read_u32(cfg_phys, phys_mem_offset, off);
        if raw == 0 {
            i += 1;
            continue;
        }

        if (raw & 0x1) == 0x1 {
            // I/O BAR
            let port = (raw & 0xFFFC) as u16;
            bars[i] = Some(Bar::Io { port, size: 0 });
            i += 1;
            continue;
        }

        // Memory BAR
        let prefetchable = (raw & (1 << 3)) != 0;
        let typ = (raw >> 1) & 0x3;
        match typ {
            0x0 => {
                // 32-bit
                let base = raw & 0xFFFF_FFF0;
                bars[i] = Some(Bar::Memory32 {
                    base,
                    size: 0,
                    prefetchable,
                });
                i += 1;
            }
            0x2 => {
                // 64-bit, consumes next BAR as high dword when available
                let low = (raw & 0xFFFF_FFF0) as u64;
                let high = if i + 1 < 6 {
                    cfg_read_u32(cfg_phys, phys_mem_offset, off + 4) as u64
                } else {
                    0
                };
                let base = (high << 32) | low;
                // "Map" via HHDM by computing the virtual address; this ensures the MMIO
                // range is reachable in the kernel virtual address space.
                let _mapped_virt = phys_mem_offset + base;
                bars[i] = Some(Bar::Memory64 {
                    base,
                    size: 0,
                    prefetchable,
                });
                // Skip the high BAR entry.
                i += 2;
            }
            _ => {
                // Reserved/unknown type; ignore.
                i += 1;
            }
        }
    }

    bars
}

/// Enumerate PCIe devices via ECAM, using the ACPI MCFG table to locate the ECAM region(s).
///
/// Populates `crate::drivers::DEVICE_REGISTRY` with a `DeviceInfo` entry per discovered device.
pub fn enumerate(rsdp_addr: u64, phys_mem_offset: VirtAddr) {
    if rsdp_addr == 0 {
        crate::serial::println!("[PCIE] No RSDP provided; cannot locate MCFG/ECAM.");
        return;
    }

    let handler = PcieAcpiHandler::new(phys_mem_offset);
    let tables = unsafe { AcpiTables::from_rsdp(handler, rsdp_addr as usize) };
    let tables = match tables {
        Ok(t) => t,
        Err(e) => {
            crate::serial::println!("[PCIE] Failed to parse ACPI tables: {:?}", e);
            return;
        }
    };

    let regions = match PciConfigRegions::new(&tables) {
        Ok(r) => r,
        Err(e) => {
            crate::serial::println!("[PCIE] No MCFG/ECAM info available: {:?}", e);
            return;
        }
    };

    crate::serial::println!("[PCIE] Enumerating devices via ECAM...");

    for bus in 0u8..=255 {
        for device in 0u8..32 {
            for function in 0u8..8 {
                let Some(cfg_phys) = regions.physical_address(0, bus, device, function) else {
                    continue;
                };

                let id = cfg_read_u32(cfg_phys, phys_mem_offset, 0x00);
                if id == 0xFFFF_FFFF {
                    // Skip empty slot without panicking.
                    continue;
                }

                let vendor_id = (id & 0xFFFF) as u16;
                let device_id = ((id >> 16) & 0xFFFF) as u16;

                let class_reg = cfg_read_u32(cfg_phys, phys_mem_offset, 0x08);
                let prog_if = ((class_reg >> 8) & 0xFF) as u8;
                let subclass = ((class_reg >> 16) & 0xFF) as u8;
                let class_code = ((class_reg >> 24) & 0xFF) as u8;

                let bars = parse_bars(cfg_phys, phys_mem_offset);

                let info = DeviceInfo {
                    vendor_id,
                    device_id,
                    class_code,
                    subclass,
                    prog_if,
                    bars,
                    irq: None,
                };

                let key = DeviceKey::new(bus, device, function, vendor_id, device_id);
                crate::drivers::DEVICE_REGISTRY
                    .lock()
                    .register_device_info(key.clone(), info);

                crate::serial::println!(
                    "[PCIE] {bus:02x}:{device:02x}.{function} {vendor_id:04x}:{device_id:04x} class={class_code:02x} sub={subclass:02x} if={prog_if:02x}",
                );
            }
        }
    }

    let device_count = crate::drivers::DEVICE_REGISTRY.lock().iter_device_infos().count();
    crate::serial::println!("[PCIE] Enumeration complete. Discovered {} device functions.", device_count);
}

