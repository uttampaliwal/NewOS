use x86_64::instructions::port::Port;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

pub fn init() {
    crate::serial::println!("[PCI] Initializing PCI driver...");
    enumerate_pci();
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

fn enumerate_pci() {
    crate::serial::println!("[PCI] Scanning buses...");
    for bus in 0..=255 {
        for slot in 0..32 {
            // Check only the first function to see if a device exists.
            let vendor = get_vendor_id(bus, slot, 0);
            if vendor != 0xFFFF {
                // Device exists. If it's a multi-function device, we should technically check all 8 functions.
                // For simplicity now, we check function 0.
                let device = get_device_id(bus, slot, 0);
                crate::serial::println!(
                    "[PCI] Found device: Bus {:02X}, Slot {:02X}, Func 00 - Vendor: {:04X}, Device: {:04X}",
                    bus, slot, vendor, device
                );
            }
        }
    }
    crate::serial::println!("[PCI] Scan complete.");
}
