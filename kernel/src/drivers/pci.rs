use x86_64::VirtAddr;
use x86_64::instructions::port::Port;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

pub fn init(phys_mem_offset: VirtAddr) {
    crate::serial::println!("[PCI] Initializing PCI driver...");
    enumerate_pci(phys_mem_offset);
}

fn pci_config_read_word(bus: u8, slot: u8, func: u8, offset: u8) -> u16 {
    let address = ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC)
        | 0x80000000;

    let mut config_addr_port: Port<u32> = Port::new(CONFIG_ADDRESS);
    let mut config_data_port: Port<u32> = Port::new(CONFIG_DATA);

    unsafe {
        config_addr_port.write(address);
        ((config_data_port.read() >> ((offset & 2) * 8)) & 0xFFFF) as u16
    }
}

fn get_vendor_id(bus: u8, slot: u8, func: u8) -> u16 {
    pci_config_read_word(bus, slot, func, 0)
}

fn get_device_id(bus: u8, slot: u8, func: u8) -> u16 {
    pci_config_read_word(bus, slot, func, 2)
}

fn get_class_code(bus: u8, slot: u8, func: u8) -> u8 {
    (pci_config_read_word(bus, slot, func, 0x0A) >> 8) as u8
}

fn get_subclass(bus: u8, slot: u8, func: u8) -> u8 {
    (pci_config_read_word(bus, slot, func, 0x0A) & 0xFF) as u8
}

fn get_bar5(bus: u8, slot: u8, func: u8) -> u64 {
    let bar5_low = pci_config_read_word(bus, slot, func, 0x24) as u64;
    let bar5_high = pci_config_read_word(bus, slot, func, 0x26) as u64;
    ((bar5_high << 16) | bar5_low) & 0xFFFFFFF0
}

fn enumerate_pci(phys_mem_offset: VirtAddr) {
    crate::serial::println!("[PCI] Scanning buses...");
    for bus in 0..=255 {
        for slot in 0..32 {
            let vendor = get_vendor_id(bus, slot, 0);
            if vendor != 0xFFFF {
                let device = get_device_id(bus, slot, 0);
                let class = get_class_code(bus, slot, 0);
                let subclass = get_subclass(bus, slot, 0);

                crate::serial::println!(
                    "[PCI] Found device: Bus {:02X}, Slot {:02X}, Func 00 - Vendor: {:04X}, Device: {:04X}, Class: {:02X}, Subclass: {:02X}",
                    bus,
                    slot,
                    vendor,
                    device,
                    class,
                    subclass
                );

                if class == 0x01 && subclass == 0x06 {
                    crate::serial::println!("[PCI] AHCI Controller found!");
                    let bar5 = get_bar5(bus, slot, 0);
                    crate::serial::println!("[PCI] AHCI BAR5: {:#x}", bar5);
                    crate::drivers::ahci::init_from_pci(bar5, phys_mem_offset);
                }
            }
        }
    }
    crate::serial::println!("[PCI] Scan complete.");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[test]
    fn test_config_address_format() {
        // Verify the PCI config address format:
        // bit 31 = 1 (enable), bus 16-23, device 11-15, function 8-10, offset 2-7
        let address = ((0u32) << 16) | ((0u32) << 11) | ((0u32) << 8) | (0u32 & 0xFC) | 0x80000000;
        assert_eq!(address, 0x80000000);
    }

    #[test]
    fn test_vendor_id_sentinel() {
        // 0xFFFF is the invalid vendor sentinel
        assert_eq!(!0u16, 0xFFFF);
    }

    #[test]
    fn test_config_offset_alignment() {
        // Config offset must be aligned to 4 bytes in the address field
        let offset = 0u8;
        assert_eq!(offset & 0xFC, 0);
        let offset = 4u8;
        assert_eq!(offset & 0xFC, 4);
        let offset = 0x0Au8;
        assert_eq!(offset & 0xFC, 8);
        let offset = 0xFFu8;
        assert_eq!(offset & 0xFC, 0xFC);
    }
}
