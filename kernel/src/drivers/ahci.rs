//! AHCI (Advanced Host Controller Interface) driver for SATA devices
//! Provides block device access using DMA.

use alloc::vec;
use alloc::vec::Vec;
use core::ptr;
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::VirtAddr;

lazy_static! {
    pub static ref AHCI_CONTROLLER: Mutex<Option<AhciController>> = Mutex::new(None);
}

/// AHCI Generic Host Control registers (offset from ABAR)
#[repr(C)]
struct Ghc {
    cap: u32,             // 0x00 - Host Capabilities
    ghc: u32,             // 0x04 - Global Host Control
    is: u32,              // 0x08 - Interrupt Status
    pi: u32,              // 0x0C - Ports Implemented
    vs: u32,              // 0x10 - Version
    ccc_ctl: u32,         // 0x14 - Command Completion Coalescing Control
    ccc_ports: u32,       // 0x18 - CCC Ports
    _reserved: [u32; 12], // 0x1C - 0x4C
    cap2: u32,            // 0x50 - Host Capabilities Extended
    bohc: u32,            // 0x54 - BIOS/OS Handoff Control
}

/// AHCI Port registers
#[repr(C)]
struct PortRegs {
    clb: u32,       // 0x00 - Command List Base Address
    clbu: u32,      // 0x04 - Command List Base Address Upper
    fb: u32,        // 0x08 - FIS Base Address
    fbu: u32,       // 0x0C - FIS Base Address Upper
    is: u32,        // 0x10 - Interrupt Status
    ie: u32,        // 0x14 - Interrupt Enable
    cmd: u32,       // 0x18 - Command and Status
    _reserved: u32, // 0x1C
    tfd: u32,       // 0x20 - Task File Data
    sig: u32,       // 0x24 - Signature
    ssts: u32,      // 0x28 - SATA Status
    sctl: u32,      // 0x2C - SATA Control
    err: u32,       // 0x30 - SATA Error
    ci: u32,        // 0x34 - Command Issue
    sact: u32,      // 0x38 - SATA Active
                    // ... more registers
}

/// AHCI Command Header
#[repr(C)]
struct CommandHeader {
    flags: u16,          // DW0: Flags and command length
    prdtl: u16,          // DW0: Physical Region Descriptor Table Length
    prdbc: u32,          // DW1: Physical Region Descriptor Byte Count
    ctba: u32,           // DW2: Command Table Base Address
    ctbau: u32,          // DW3: Command Table Base Address Upper
    _reserved: [u32; 4], // DW4-7: Reserved
}

/// AHCI PRD Table Entry
#[repr(C)]
struct PrdEntry {
    dba: u32,       // Data Base Address
    dbau: u32,      // Data Base Address Upper
    _reserved: u32, // Reserved
    dbc: u32,       // Byte Count (bit 31 = interrupt)
}

/// AHCI Controller structure
pub struct AhciController {
    ghc: &'static mut Ghc,
    ports: &'static mut [PortRegs; 32],
    #[allow(dead_code)]
    abar: VirtAddr,
    cmd_list: Vec<u8>,  // Command list memory
    recv_fis: Vec<u8>,  // Receive FIS memory
    cmd_table: Vec<u8>, // Command table memory
}

impl AhciController {
    /// Initialize a port for DMA
    fn init_port_dma(&mut self, port_num: usize) {
        let port = &mut self.ports[port_num];

        // Stop command engine
        port.cmd &= !0x00000001; // Clear ST (Start)
        loop {
            if port.cmd & 0x00000001 == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        // Stop FIS reception
        port.cmd &= !0x00000010; // Clear FRE (FIS Receive Enable)
        loop {
            if port.cmd & 0x00000010 == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        // Allocate and set command list base (1KB aligned)
        let cmd_list_addr = self.cmd_list.as_ptr() as u64;
        port.clb = cmd_list_addr as u32;
        port.clbu = (cmd_list_addr >> 32) as u32;

        // Allocate and set FIS base (256-byte aligned)
        let recv_fis_addr = self.recv_fis.as_ptr() as u64;
        port.fb = recv_fis_addr as u32;
        port.fbu = (recv_fis_addr >> 32) as u32;

        // Clear command list
        // Safety: cmd_list, recv_fis, and cmd_table are valid heap allocations owned by self.
        unsafe {
            ptr::write_bytes(self.cmd_list.as_mut_ptr(), 0, self.cmd_list.len());
            ptr::write_bytes(self.recv_fis.as_mut_ptr(), 0, self.recv_fis.len());
            ptr::write_bytes(self.cmd_table.as_mut_ptr(), 0, self.cmd_table.len());
        }

        // Start FIS reception
        port.cmd |= 0x00000010; // Set FRE

        // Start command engine
        port.cmd |= 0x00000001; // Set ST

        crate::serial::println!("[AHCI] Port {} initialized for DMA", port_num);
    }

    /// Read blocks using DMA
    pub fn read_blocks(
        &mut self,
        port_num: usize,
        lba: u64,
        count: usize,
        buffer: &mut [u8],
    ) -> bool {
        if port_num >= 32 {
            return false;
        }

        let port = &mut self.ports[port_num];
        let sig = port.sig;

        // Check if this is a SATA device
        if sig != 0x00000101 {
            crate::serial::println!(
                "[AHCI] Port {} is not a SATA device (sig: {:#x})",
                port_num,
                sig
            );
            return false;
        }

        // Set up command header
        // Safety: cmd_list is a valid heap allocation of at least sizeof(CommandHeader) bytes.
        let cmd_header = unsafe { &mut *(self.cmd_list.as_mut_ptr() as *mut CommandHeader) };

        // Configure command: FIS length = 5 DWORDS, write = 0 (read), prefetchable = 1
        cmd_header.flags = 5 | (1 << 7); // FIS length | C (Clear busy)
        cmd_header.prdtl = 1; // One PRD entry
        cmd_header.prdbc = 0;
        cmd_header.ctba = self.cmd_table.as_ptr() as u32;
        cmd_header.ctbau = (self.cmd_table.as_ptr() as u64 >> 32) as u32;

        // Set up command table - H2D FIS (Register FIS)
        // Safety: cmd_table is a valid heap allocation of at least sizeof(H2dFis) bytes.
        let cmd_fis = unsafe { &mut *(self.cmd_table.as_mut_ptr() as *mut H2dFis) };

        // Build H2D FIS for READ DMA EXT (25h) or READ SECTOR(S) EXT (24h)
        // For simplicity, using READ DMA (C8h) for smaller transfers
        cmd_fis.fis_type = 0x27; // H2D FIS type
        cmd_fis.flags = 0x80; // Command bit set
        cmd_fis.command = 0xC8; // READ DMA
        cmd_fis.lba_low = (lba & 0xFFFFFF) as u32;
        cmd_fis.lba_mid = ((lba >> 24) & 0xFFFFFF) as u32;
        cmd_fis.lba_high = ((lba >> 48) & 0xFFFFFF) as u32;
        cmd_fis.device = 0x40; // LBA mode
        cmd_fis.count = count as u16;
        cmd_fis.icc = 0;

        // Set up PRD table (in command table, after FIS)
        let prd_offset = 0x80; // FIS is 0x40 bytes, PRD table starts after
        // Safety: cmd_table has sufficient size for PRD entry at offset 0x80.
        let prd_entry =
            unsafe { &mut *(self.cmd_table.as_mut_ptr().add(prd_offset) as *mut PrdEntry) };

        prd_entry.dba = buffer.as_ptr() as u32;
        prd_entry.dbau = (buffer.as_ptr() as u64 >> 32) as u32;
        prd_entry.dbc = (buffer.len() as u32 - 1) | (1 << 31); // Interrupt on completion

        // Issue command
        port.ci |= 1; // Set bit 0 in Command Issue

        // Wait for completion
        let mut timeout = 1000000; // Large timeout
        while port.ci & 1 != 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        if timeout == 0 {
            crate::serial::println!("[AHCI] Timeout reading blocks from port {}", port_num);
            return false;
        }

        crate::serial::println!(
            "[AHCI] Read {} blocks from LBA {} on port {}",
            count,
            lba,
            port_num
        );
        true
    }
}

/// H2D FIS (Host to Device Register FIS)
#[repr(C)]
struct H2dFis {
    fis_type: u8,  // 0x27 for H2D
    flags: u8,     // Bit 7 = C (Command), Bit 6 = P (Control)
    command: u8,   // Command register
    features: u8,  // Features register
    lba_low: u32,  // LBA 0-23 and 24-31
    lba_mid: u32,  // LBA 32-47 and 48-55
    lba_high: u32, // LBA 48-63 and 64-71
    device: u8,    // Device register
    count: u16,    // Sector count
    icc: u8,       // Isochronous command completion
    control: u8,   // Control register
    _reserved: [u8; 4],
}

/// Initialize AHCI controller
fn init(abar: VirtAddr) -> Option<AhciController> {
    // Safety: abar is a valid AHCI ABAR address mapped via PCI BAR5.
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
    // Safety: ports_base points to valid AHCI port register space within ABAR.
    let ports = unsafe { &mut *(ports_base.as_mut_ptr::<[PortRegs; 32]>()) };

    // Allocate DMA buffers (must be 1KB aligned for command list, 256-byte for FIS)
    let cmd_list = vec![0u8; 1024]; // 1KB for command list
    let recv_fis = vec![0u8; 256]; // 256 bytes for receive FIS
    let cmd_table = vec![0u8; 8192]; // 8KB for command table

    Some(AhciController {
        ghc,
        ports,
        abar,
        cmd_list,
        recv_fis,
        cmd_table,
    })
}

/// Initialize AHCI from PCI BAR
pub fn init_from_pci(bar5_addr: u64, phys_mem_offset: VirtAddr) -> bool {
    if bar5_addr == 0 {
        crate::serial::println!("[AHCI] No ABAR found (BAR5 is 0)");
        return false;
    }

    let abar = phys_mem_offset + bar5_addr;
    crate::serial::println!(
        "[AHCI] ABAR at: {:#x}, virt: {:#x}",
        bar5_addr,
        abar.as_u64()
    );

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
    drop(guard);

    // Probe ports
    if let Some(ref mut ctrl) = AHCI_CONTROLLER.lock().as_mut() {
        // Initialize port 0 if it has a SATA device
        let ports_impl = ctrl.ghc.pi;
        if ports_impl & 1 != 0 {
            let port = &mut ctrl.ports[0];
            let sig = port.sig;
            if sig == 0x00000101 {
                ctrl.init_port_dma(0);
            }
        }
    }

    true
}

/// Read blocks from AHCI device
pub fn read_blocks(_device: usize, lba: u64, count: usize, buffer: &mut [u8]) -> bool {
    let mut guard = AHCI_CONTROLLER.lock();
    if let Some(ref mut ctrl) = guard.as_mut() {
        // For now, assume device 0 = port 0
        ctrl.read_blocks(0, lba, count, buffer)
    } else {
        crate::serial::println!("[AHCI] Controller not initialized");
        false
    }
}
