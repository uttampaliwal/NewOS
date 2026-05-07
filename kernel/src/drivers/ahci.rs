//! AHCI (Advanced Host Controller Interface) driver for SATA devices
//! This provides block device access for storage

use spin::Mutex;
use x86_64::VirtAddr;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref AHCI_CONTROLLER: Mutex<Option<AhciController>> = Mutex::new(None);
}

/// AHCI Generic Host Control registers (offset from ABAR)
#[repr(C)]
struct Ghc {
    cap: u32,        // 0x00 - Host Capabilities
    ghc: u32,        // 0x04 - Global Host Control
    is: u32,          // 0x08 - Interrupt Status
    pi: u32,          // 0x0C - Ports Implemented
    vs: u32,          // 0x10 - Version
    ccc_ctl: u32,     // 0x14 - Command Completion Coalescing Control
    ccc_ports: u32,   // 0x18 - CCC Ports
    _reserved: [u32; 12], // 0x1C - 0x4C
    cap2: u32,       // 0x50 - Host Capabilities Extended
    bohc: u32,       // 0x54 - BIOS/OS Handoff Control
}

/// AHCI Port registers
#[repr(C)]
struct PortRegs {
    clb: u32,         // 0x00 - Command List Base Address
    clbu: u32,        // 0x04 - Command List Base Address Upper
    fb: u32,          // 0x08 - FIS Base Address
    fbu: u32,         // 0x0C - FIS Base Address Upper
    is: u32,          // 0x10 - Interrupt Status
    ie: u32,          // 0x14 - Interrupt Enable
    cmd: u32,         // 0x18 - Command and Status
    _reserved: u32,   // 0x1C
    tfd: u32,         // 0x20 - Task File Data
    sig: u32,         // 0x24 - Signature
    ssts: u32,        // 0x28 - SATA Status
    sctl: u32,        // 0x2C - SATA Control
    err: u32,         // 0x30 - SATA Error
    ci: u32,          // 0x34 - Command Issue
    sact: u32,        // 0x38 - SATA Active
    // ... more registers
}

/// AHCI Controller structure
pub struct AhciController {
    ghc: &'static mut Ghc,
    ports: &'static mut [PortRegs; 32],
    abar: VirtAddr,
}

/// Initialize AHCI controller
pub fn init(abar: VirtAddr) -> Option<AhciController> {
    let ghc = unsafe { &mut *(abar.as_mut_ptr::<Ghc>()) };
    
    // Check AHCI version
    let major = (ghc.vs >> 16) & 0xFFFF;
    let minor = ghc.vs & 0xFFFF;
    crate::serial::println!("[AHCI] Version: {}.{}", major, minor);
    
    // Enable AHCI (set AE bit)
    ghc.ghc |= 0x80000000;
    
    // Get implemented ports
    let ports_impl = ghc.pi;
    crate::serial::println!("[AHCI] Implemented ports: {:#x}", ports_impl);
    
    // Calculate port registers base (0x100 from ABAR)
    let ports_base = abar + 0x100;
    let ports = unsafe { &mut *(ports_base.as_mut_ptr::<[PortRegs; 32]>()) };
    
    Some(AhciController {
        ghc,
        ports,
        abar,
    })
}

impl AhciController {
    /// Probe for SATA devices on all implemented ports
    pub fn probe_ports(&mut self) {
        let ports_impl = self.ghc.pi;
        
        for port_num in 0..32 {
            if (ports_impl >> port_num) & 1 == 0 {
                continue;
            }
            
            let port = &mut self.ports[port_num as usize];
            
            // Check port type (SATA = 0x101)
            let sig = port.sig;
            if sig == 0x101 {
                crate::serial::println!("[AHCI] Port {}: SATA device detected", port_num);
                self.init_port(port_num as usize);
            } else if sig == 0xEB140101 {
                crate::serial::println!("[AHCI] Port {}: SATAPI device detected", port_num);
            } else if sig == 0x96690101 {
                crate::serial::println!("[AHCI] Port {}: SEMB device detected", port_num);
            } else if sig == 0x00000101 {
                crate::serial::println!("[AHCI] Port {}: PMP device detected", port_num);
            } else {
                crate::serial::println!("[AHCI] Port {}: No device (sig: {:#x})", port_num, sig);
            }
        }
    }
    
    fn init_port(&mut self, port_num: usize) {
        let port = &mut self.ports[port_num];
        
        // Stop command engine
        port.cmd &= !0x00000001; // Clear ST
        while port.cmd & 0x00000001 != 0 {
            core::hint::spin_loop();
        }
        
        // Stop FIS reception
        port.cmd &= !0x00000010; // Clear FRE
        while port.cmd & 0x00000010 != 0 {
            core::hint::spin_loop();
        }
        
        // Set FIS base address (needs to be implemented)
        // Set command list base address (needs to be implemented)
        
        // Start FIS reception
        port.cmd |= 0x00000010; // Set FRE
        
        // Start command engine
        port.cmd |= 0x00000001; // Set ST
        
        crate::serial::println!("[AHCI] Port {} initialized", port_num);
    }
}

/// Initialize AHCI from PCI BAR
pub fn init_from_pci(bar5_addr: u64, phys_mem_offset: VirtAddr) -> bool {
    if bar5_addr == 0 {
        crate::serial::println!("[AHCI] No ABAR found (BAR5 is 0)");
        return false;
    }
    
    let abar = phys_mem_offset + bar5_addr;
    crate::serial::println!("[AHCI] ABAR at: {:#x}, virt: {:#x}", bar5_addr, abar.as_u64());
    
    let controller = match init(abar) {
        Some(c) => c,
        None => {
            crate::serial::println!("[AHCI] Failed to initialize controller");
            return false;
        }
    };
    
    // Store controller and probe ports
    let mut guard = AHCI_CONTROLLER.lock();
    *guard = Some(controller);
    drop(guard); // Release lock before probing
    
    if let Some(ref mut ctrl) = *AHCI_CONTROLLER.lock() {
        ctrl.probe_ports();
    }
    
    true
}

/// Read blocks from AHCI device (stub - needs proper implementation)
pub fn read_blocks(_device: usize, _lba: u64, _count: usize, _buffer: &mut [u8]) -> bool {
    // Stub implementation - would use AHCI DMA to read blocks
    crate::serial::println!("[AHCI] read_blocks stub: device={}, lba={}, count={}", _device, _lba, _count);
    false
}
