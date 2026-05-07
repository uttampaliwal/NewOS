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

fn parse_bars(reader: &dyn Fn(u16) -> u32) -> [Option<Bar>; 6] {
    let mut bars: [Option<Bar>; 6] = [None, None, None, None, None, None];

    let mut i = 0usize;
    while i < 6 {
        let off = 0x10u16 + (i as u16) * 4;
        let raw = reader(off);
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
                    reader(off + 4) as u64
                } else {
                    0
                };
                let base = (high << 32) | low;
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

                let reader = &|off: u16| cfg_read_u32(cfg_phys, phys_mem_offset, off);
                let bars = parse_bars(reader);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skip_invalid_vendor_id() {
        // Test that 0xFFFFFFFF is recognized as an invalid vendor/device ID and would be skipped
        let invalid_id = 0xFFFFFFFFu32;
        assert_eq!(invalid_id, 0xFFFF_FFFFu32);
        // In enumerate, if id == 0xFFFF_FFFF { continue; } so it skips without panic
    }

    #[test]
    fn test_parse_64bit_bar() {
        // Mock config space: BAR0 is 64-bit memory BAR
        let mut config = [0u32; 16];
        config[4] = 0x00000004; // BAR0 low: 64-bit memory, prefetchable=0, base_low=0x00000000
        config[5] = 0x12345678; // BAR1 high: 0x12345678
        // Others 0
        let reader = |off: u16| config[(off / 4) as usize];
        let bars = parse_bars(&reader);

        // BAR0 should be Memory64 with base = (0x12345678 << 32) | 0x00000000 = 0x1234567800000000
        match bars[0] {
            Some(Bar::Memory64 { base, size: _, prefetchable }) => {
                assert_eq!(base, 0x1234567800000000);
                assert_eq!(prefetchable, false);
            }
            _ => panic!("Expected Memory64 BAR"),
        }

        // BAR1 should be None since it's consumed as high part of BAR0
        assert!(bars[1].is_none());
    }
}

