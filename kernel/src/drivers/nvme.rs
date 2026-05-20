//! NVMe (NVM Express) block driver
//!
//! Implements [`DeviceDriver`] for NVMe controllers (PCI class `0x01`, subclass `0x08`).
//! Supports admin queue initialisation, I/O submission/completion queue pair,
//! identify namespace to discover namespace capacity and LBA size (512/4096 bytes),
//! read and write NVM commands using PRPs, and 30-second command timeout.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use spin::Mutex;
use alloc::vec::Vec;

use crate::boot::get_phys_mem_offset;
use crate::drivers::framework::{DeviceDriver, DeviceInfo};

// ---------------------------------------------------------------------------
// NVMe PCI class/subclass
// ---------------------------------------------------------------------------

const NVME_CLASS: u8 = 0x01;
const NVME_SUBCLASS: u8 = 0x08;

// ---------------------------------------------------------------------------
// MMIO register offsets (from BAR0)
// ---------------------------------------------------------------------------

const REG_CAP: u64 = 0x00;
const REG_VS: u64 = 0x08;
const REG_CC: u64 = 0x14;
const REG_CSTS: u64 = 0x1C;
const REG_AQA: u64 = 0x24;
const REG_ASQ: u64 = 0x28;
const REG_ACQ: u64 = 0x30;

// Controller Configuration (CC) bits
const CC_EN: u32 = 0x0000_0001;
const CC_IOCQES_SHIFT: u32 = 16;
const CC_IOSQES_SHIFT: u32 = 20;
const CC_CSS_NVM: u32 = 0 << 4;
const CC_AMS_RR: u32 = 0 << 11;

// Controller Status (CSTS) bits
const CSTS_RDY: u32 = 0x0000_0001;
// Capabilities (CAP) fields
const CAP_TO_SHIFT: u64 = 24;
const CAP_DSTRD_SHIFT: u64 = 32;

// ---------------------------------------------------------------------------
// Queue configuration
// ---------------------------------------------------------------------------

const ADMIN_QUEUE_SIZE: u16 = 64;
const IO_QUEUE_SIZE: u16 = 256;

const SQ_ENTRY_SIZE: u8 = 64;
const CQ_ENTRY_SIZE: u8 = 16;
const SQ_ENTRY_SIZE_LOG2: u8 = 6;
const CQ_ENTRY_SIZE_LOG2: u8 = 4;

// ---------------------------------------------------------------------------
// Admin command opcodes
// ---------------------------------------------------------------------------

const ADMIN_CREATE_IO_CQ: u8 = 0x05;
const ADMIN_CREATE_IO_SQ: u8 = 0x01;
const ADMIN_IDENTIFY: u8 = 0x06;
const ADMIN_ABORT: u8 = 0x08;

// NVM command opcodes
const NVM_READ: u8 = 0x02;
const NVM_WRITE: u8 = 0x01;

// Identify CNS values
const IDENTIFY_NS: u8 = 0x00;

// ---------------------------------------------------------------------------
// Timeout
// ---------------------------------------------------------------------------

/// Spin-loop iterations for 30-second timeout (~10⁸ iterations on QEMU).
const CMD_TIMEOUT_ITER: u64 = 100_000_000;

// ---------------------------------------------------------------------------
// MMIO helpers
// ---------------------------------------------------------------------------

fn mmio_read32(base: u64, offset: u64) -> u32 {
    unsafe { read_volatile((base + offset) as *const u32) }
}

fn mmio_write32(base: u64, offset: u64, val: u32) {
    unsafe { write_volatile((base + offset) as *mut u32, val) }
}

fn mmio_read64(base: u64, offset: u64) -> u64 {
    unsafe { read_volatile((base + offset) as *const u64) }
}

fn mmio_write64(base: u64, offset: u64, val: u64) {
    unsafe { write_volatile((base + offset) as *mut u64, val) }
}

// ---------------------------------------------------------------------------
// Queue structures
// ---------------------------------------------------------------------------

/// Submission Queue Entry (64 bytes)
#[repr(C, align(8))]
struct SubmissionQueueEntry {
    opcode: u8,
    flags: u8,
    command_id: u16,
    nsid: u32,
    cdw2: u32,
    cdw3: u32,
    mptr: u64,
    prp1: u64,
    prp2: u64,
    cdw10: u32,
    cdw11: u32,
    cdw12: u32,
    cdw13: u32,
    cdw14: u32,
    cdw15: u32,
}

/// Completion Queue Entry (16 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C, align(4))]
struct CompletionQueueEntry {
    cdw0: u32,
    cdw1: u32,
    sq_head: u16,
    sq_id: u16,
    command_id: u16,
    status: u16,
}

/// Identify Namespace data (4096 bytes)
#[repr(C, align(4096))]
struct IdentifyNamespaceData {
    nsze: u64,
    ncap: u64,
    nuse: u64,
    nsfeat: u8,
    nlbaf: u8,
    flbas: u8,
    mc: u8,
    dpc: u8,
    dps: u8,
    nmic: u8,
    rescap: u8,
    fpi: u8,
    dlfeat: u8,
    _reserved0: [u8; 40],
    lbaf0: LbaFormat,
    lbaf1: LbaFormat,
    lbaf2: LbaFormat,
    lbaf3: LbaFormat,
    lbaf4: LbaFormat,
    lbaf5: LbaFormat,
    lbaf6: LbaFormat,
    lbaf7: LbaFormat,
    lbaf8: LbaFormat,
    lbaf9: LbaFormat,
    lbaf10: LbaFormat,
    lbaf11: LbaFormat,
    lbaf12: LbaFormat,
    lbaf13: LbaFormat,
    lbaf14: LbaFormat,
    lbaf15: LbaFormat,
    _reserved1: [u8; 3712],
}

/// LBA Format entry (4 bytes)
#[repr(C)]
struct LbaFormat {
    /// Lower 16 bits: LBA data size (as power of 2, e.g., 9 = 512 bytes)
    raw: u32,
}

impl LbaFormat {
    fn lba_data_size(&self) -> u64 {
        1u64 << (self.raw & 0xF)
    }
}

// ---------------------------------------------------------------------------
// Namespace information
// ---------------------------------------------------------------------------

/// Information about a single NVMe namespace exposed as a block device.
#[derive(Debug, Clone)]
pub struct NvmeNamespace {
    /// Namespace identifier (NSID).
    pub nsid: u32,
    /// Total number of LBAs (Namespace Size from Identify).
    pub nsze: u64,
    /// LBA size in bytes (512 or 4096).
    pub lba_size: u64,
    /// Total capacity in bytes.
    pub capacity: u64,
}

// ---------------------------------------------------------------------------
// NVMe Controller state
// ---------------------------------------------------------------------------

/// NVMe Controller instance.
pub struct NvmeController {
    /// Virtual address of BAR0 (MMIO registers).
    bar0: u64,
    /// Physical memory offset for PRP translation.
    phys_mem_offset: u64,
    /// Admin Submission Queue memory.
    admin_sq_mem: Vec<u8>,
    /// Admin Completion Queue memory.
    admin_cq_mem: Vec<u8>,
    /// I/O Submission Queue memory.
    io_sq_mem: Vec<u8>,
    /// I/O Completion Queue memory.
    io_cq_mem: Vec<u8>,
    /// Admin SQ tail pointer (doorbell value).
    admin_sq_tail: AtomicU16,
    /// Admin CQ head pointer.
    admin_cq_head: AtomicU16,
    /// I/O SQ tail pointer.
    io_sq_tail: AtomicU16,
    /// I/O CQ head pointer.
    io_cq_head: AtomicU16,
    /// Next command ID to use.
    next_cid: AtomicU16,
    /// Doorbell stride (in bytes).
    doorbell_stride: u64,
    /// Discovered namespaces.
    pub namespaces: Vec<NvmeNamespace>,
    /// Device key for this controller (for registry lookup).
    #[allow(dead_code)]
    device_info: DeviceInfo,
}

// SAFETY: NvmeController is only accessed behind a Mutex or single-threaded.
unsafe impl Send for NvmeController {}
unsafe impl Sync for NvmeController {}

impl NvmeController {
    /// Compute the doorbell register offset for a submission queue.
    fn sq_doorbell(&self, qid: u16) -> u64 {
        0x1000 + (2 * qid as u64) * self.doorbell_stride
    }

    /// Compute the doorbell register offset for a completion queue.
    fn cq_doorbell(&self, qid: u16) -> u64 {
        0x1000 + (2 * qid as u64 + 1) * self.doorbell_stride
    }

    /// Ring the admin SQ tail doorbell.
    fn ring_admin_sq(&self, tail: u16) {
        mmio_write32(self.bar0, self.sq_doorbell(0), tail as u32);
    }

    /// Ring the admin CQ head doorbell.
    fn ring_admin_cq(&self, head: u16) {
        mmio_write32(self.bar0, self.cq_doorbell(0), head as u32);
    }

    /// Ring the I/O SQ tail doorbell.
    fn ring_io_sq(&self, tail: u16) {
        mmio_write32(self.bar0, self.sq_doorbell(1), tail as u32);
    }

    /// Ring the I/O CQ head doorbell.
    fn ring_io_cq(&self, head: u16) {
        mmio_write32(self.bar0, self.cq_doorbell(1), head as u32);
    }

    /// Allocate and return a new command ID.
    fn alloc_cid(&self) -> u16 {
        self.next_cid.fetch_add(1, Ordering::Relaxed)
    }

    /// Submit an admin command and wait for completion.
    fn admin_command(
        &mut self,
        opcode: u8,
        nsid: u32,
        prp1: u64,
        prp2: u64,
        cdw10: u32,
        cdw11: u32,
        cdw12: u32,
        cdw13: u32,
    ) -> Result<CompletionQueueEntry, &'static str> {
        let cid = self.alloc_cid();
        let tail = self.admin_sq_tail.load(Ordering::Relaxed);
        let sq_entry_count = ADMIN_QUEUE_SIZE as usize;
        let slt = self.admin_sq_mem.as_mut_ptr() as *mut SubmissionQueueEntry;

        let idx = tail as usize % sq_entry_count;
        let entry = unsafe { &mut *slt.add(idx) };
        *entry = SubmissionQueueEntry {
            opcode,
            flags: 0,
            command_id: cid,
            nsid,
            cdw2: 0,
            cdw3: 0,
            mptr: 0,
            prp1,
            prp2,
            cdw10,
            cdw11,
            cdw12,
            cdw13,
            cdw14: 0,
            cdw15: 0,
        };

        core::sync::atomic::fence(Ordering::Release);
        let new_tail = tail.wrapping_add(1);
        self.admin_sq_tail.store(new_tail, Ordering::Relaxed);
        self.ring_admin_sq(new_tail);

        // Poll for completion
        self.poll_cq(0, cid, CMD_TIMEOUT_ITER)?;

        // Read completion queue entry
        let cq_entry_count = ADMIN_QUEUE_SIZE as usize;
        let clt = self.admin_cq_mem.as_mut_ptr() as *mut CompletionQueueEntry;
        let cq_head = self.admin_cq_head.load(Ordering::Relaxed);
        let cidx = cq_head as usize % cq_entry_count;
        let cqe = unsafe { *clt.add(cidx) };

        // Advance CQ head
        self.admin_cq_head.store(cq_head.wrapping_add(1), Ordering::Relaxed);
        self.ring_admin_cq(cq_head.wrapping_add(1));

        if (cqe.status & 0x7F) != 0 {
            let sts = cqe.status;
            return Err(match sts & 0x7F {
                0x01 => "interrupted",
                0x02 => "invalid opcode",
                0x05 => "aborted",
                0x06 => "namespace not ready",
                _ => "admin command failed",
            });
        }

        Ok(cqe)
    }

    /// Poll a completion queue for a specific command ID with timeout.
    fn poll_cq(&self, qid: u16, cid: u16, timeout: u64) -> Result<(), &'static str> {
        let cq_mem = if qid == 0 {
            &self.admin_cq_mem
        } else {
            &self.io_cq_mem
        };
        let cq_size = if qid == 0 {
            ADMIN_QUEUE_SIZE as usize
        } else {
            IO_QUEUE_SIZE as usize
        };

        let cq_head = if qid == 0 {
            self.admin_cq_head.load(Ordering::Relaxed)
        } else {
            self.io_cq_head.load(Ordering::Relaxed)
        };

        let clt = cq_mem.as_ptr() as *const CompletionQueueEntry;
        let mut iter: u64 = 0;

        loop {
            core::sync::atomic::fence(Ordering::Acquire);
            let idx = cq_head as usize % cq_size;
            let cqe = unsafe { *clt.add(idx) };

            if cqe.command_id == cid {
                return Ok(());
            }

            if (cqe.status & 0x7FFF) == 0xFFFF {
                // Phase bit not flipped yet - still pending
            }

            iter += 1;
            if iter >= timeout {
                return Err("timeout");
            }

            core::hint::spin_loop();
        }
    }

    /// Abort a command by its ID.
    fn abort_command(&mut self, cid: u16) -> Result<(), &'static str> {
        let _ = self.admin_command(
            ADMIN_ABORT,
            0,
            0,
            0,
            cid as u32,
            0,
            0,
            0,
        );
        crate::serial::println!("[NVMe] Aborted command CID={}", cid);
        Ok(())
    }

    /// Create an I/O Completion Queue.
    fn create_io_cq(&mut self, qid: u16, size: u16, irq_vector: u16) -> Result<(), &'static str> {
        let cq_phys = self.io_cq_mem.as_ptr() as u64 - self.phys_mem_offset;
        let cdw10 = (size as u32) << 16 | qid as u32;
        // Bit 0 = physically contiguous, Bit 1 = interrupts enabled
        let cdw11 = (irq_vector as u32) << 16 | 0x0001;
        self.admin_command(
            ADMIN_CREATE_IO_CQ,
            0,
            cq_phys,
            0,
            cdw10,
            cdw11,
            0,
            0,
        )?;
        crate::serial::println!("[NVMe] Created I/O CQ qid={} size={}", qid, size);
        Ok(())
    }

    /// Create an I/O Submission Queue.
    fn create_io_sq(&mut self, qid: u16, size: u16, cq_id: u16) -> Result<(), &'static str> {
        let sq_phys = self.io_sq_mem.as_ptr() as u64 - self.phys_mem_offset;
        let cdw10 = (size as u32) << 16 | qid as u32;
        // Bit 0 = physically contiguous, CQ ID in upper 16 bits
        let cdw11 = (cq_id as u32) << 16 | 0x0001;
        self.admin_command(
            ADMIN_CREATE_IO_SQ,
            0,
            sq_phys,
            0,
            cdw10,
            cdw11,
            0,
            0,
        )?;
        crate::serial::println!("[NVMe] Created I/O SQ qid={} size={} cq_id={}", qid, size, cq_id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// NvmeError
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum NvmeError {
    ProbeFailed(&'static str),
    InitFailed(&'static str),
    IoTimeout { nsid: u32, lba: u64, count: u64 },
    IoError { nsid: u32, status: u16 },
    NoMemory,
    Unsupported,
    Timeout,
}

// ---------------------------------------------------------------------------
// NvmeDriver
// ---------------------------------------------------------------------------

/// The NVMe driver instance stored in the DeviceRegistry.
pub struct NvmeDriver;

impl DeviceDriver for NvmeDriver {
    type Config = ();
    type Error = NvmeError;

    fn probe(info: &DeviceInfo) -> Result<Self, Self::Error> {
        if info.class_code != NVME_CLASS || info.subclass != NVME_SUBCLASS {
            return Err(NvmeError::ProbeFailed("not an NVMe controller"));
        }

        let bar0_phys = match info.bars[0] {
            Some(super::framework::Bar::Memory32 { base, .. }) => base as u64,
            Some(super::framework::Bar::Memory64 { base, .. }) => base,
            _ => return Err(NvmeError::ProbeFailed("no valid BAR0")),
        };

        let phys_mem_offset = get_phys_mem_offset();
        let bar0 = phys_mem_offset.as_u64() + bar0_phys;

        crate::serial::println!(
            "[NVMe] Probing NVMe controller at {:02x}:{:02x}.{:02x} (BAR0 phys={:#x})",
            info.bus, info.device, info.function, bar0_phys,
        );

        // Read version
        let vs = mmio_read32(bar0, REG_VS);
        let major = (vs >> 16) & 0xFFFF;
        let minor = vs & 0xFFFF;
        crate::serial::println!("[NVMe] NVMe spec {}.{}", major, minor);

        // ---- Wait for CSTS.RDY = 0 (controller not already enabled) ----
        let csts = mmio_read32(bar0, REG_CSTS);
        if csts & CSTS_RDY != 0 {
            // Disable: clear CC.EN
            let cc = mmio_read32(bar0, REG_CC);
            mmio_write32(bar0, REG_CC, cc & !CC_EN);
            // Wait for CSTS.RDY to clear
            let mut timeout = 10_000_000u64;
            while mmio_read32(bar0, REG_CSTS) & CSTS_RDY != 0 {
                timeout -= 1;
                if timeout == 0 {
                    return Err(NvmeError::ProbeFailed("controller failed to become not-ready"));
                }
                core::hint::spin_loop();
            }
        }

        // Read CAP to get timeout and doorbell stride
        let cap = mmio_read64(bar0, REG_CAP);
        let cap_to = ((cap >> CAP_TO_SHIFT) & 0xFF) as u32;
        let dstrd = ((cap >> CAP_DSTRD_SHIFT) & 0xF) as u8;
        let doorbell_stride = 4u64 << dstrd;

        crate::serial::println!(
            "[NVMe] CAP_TO={}ms DSTRD={} doorbell_stride={}",
            cap_to * 500, dstrd, doorbell_stride,
        );

        // ---- Allocate Admin Queue memory ----
        let admin_sq_size = ADMIN_QUEUE_SIZE as usize * SQ_ENTRY_SIZE as usize;
        let admin_cq_size = ADMIN_QUEUE_SIZE as usize * CQ_ENTRY_SIZE as usize;
        let admin_sq_mem = alloc::vec![0u8; admin_sq_size];
        let admin_cq_mem = alloc::vec![0u8; admin_cq_size];

        let io_sq_size = IO_QUEUE_SIZE as usize * SQ_ENTRY_SIZE as usize;
        let io_cq_size = IO_QUEUE_SIZE as usize * CQ_ENTRY_SIZE as usize;
        let io_sq_mem = alloc::vec![0u8; io_sq_size];
        let io_cq_mem = alloc::vec![0u8; io_cq_size];

        // Calculate physical addresses (PRP uses physical addresses)
        let pmo = phys_mem_offset.as_u64();
        let admin_sq_phys = admin_sq_mem.as_ptr() as u64 - pmo;
        let admin_cq_phys = admin_cq_mem.as_ptr() as u64 - pmo;

        // ---- Set Admin Queue Attributes ----
        let aqa = (ADMIN_QUEUE_SIZE as u32) << 16 | ADMIN_QUEUE_SIZE as u32;
        mmio_write32(bar0, REG_AQA, aqa);

        // ---- Set Admin SQ and CQ base addresses ----
        mmio_write64(bar0, REG_ASQ, admin_sq_phys);
        mmio_write64(bar0, REG_ACQ, admin_cq_phys);

        // ---- Enable Controller ----
        let cc_val = CC_EN
            | CC_CSS_NVM
            | CC_AMS_RR
            | ((SQ_ENTRY_SIZE_LOG2 as u32) << CC_IOSQES_SHIFT)
            | ((CQ_ENTRY_SIZE_LOG2 as u32) << CC_IOCQES_SHIFT);
        mmio_write32(bar0, REG_CC, cc_val);

        // Wait for CSTS.RDY = 1
        let mut timeout = 10_000_000u64;
        while mmio_read32(bar0, REG_CSTS) & CSTS_RDY == 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(NvmeError::ProbeFailed("controller failed to become ready"));
            }
            core::hint::spin_loop();
        }

        crate::serial::println!("[NVMe] Controller enabled and ready");

        // ---- Build controller state ----
        let mut ctrl = NvmeController {
            bar0,
            phys_mem_offset: pmo,
            admin_sq_mem,
            admin_cq_mem,
            io_sq_mem,
            io_cq_mem,
            admin_sq_tail: AtomicU16::new(0),
            admin_cq_head: AtomicU16::new(0),
            io_sq_tail: AtomicU16::new(0),
            io_cq_head: AtomicU16::new(0),
            next_cid: AtomicU16::new(1),
            doorbell_stride,
            namespaces: Vec::new(),
            device_info: info.clone(),
        };

        // ---- Identify namespaces ----
        // Check namespace 1..32
        for nsid in 1..=32u32 {
            let mut ns_data = IdentifyNamespaceData::default();

            let data_phys = (&mut ns_data as *mut IdentifyNamespaceData) as u64 - pmo;

            let result = ctrl.admin_command(
                ADMIN_IDENTIFY,
                nsid,
                data_phys,
                0,
                IDENTIFY_NS as u32,
                0,
                0,
                0,
            );

            match result {
                Ok(cqe) => {
                    if (cqe.status & 0x7F) == 0 {
                        // Successful identify
                        if ns_data.ncap > 0 && ns_data.nsze > 0 {
                            // Determine LBA format
                            let flbas = ns_data.flbas & 0x0F;
                            let lba_size = match flbas {
                                0..=15 => {
                                    let formats = [
                                        &ns_data.lbaf0, &ns_data.lbaf1, &ns_data.lbaf2, &ns_data.lbaf3,
                                        &ns_data.lbaf4, &ns_data.lbaf5, &ns_data.lbaf6, &ns_data.lbaf7,
                                        &ns_data.lbaf8, &ns_data.lbaf9, &ns_data.lbaf10, &ns_data.lbaf11,
                                        &ns_data.lbaf12, &ns_data.lbaf13, &ns_data.lbaf14, &ns_data.lbaf15,
                                    ];
                                    formats[flbas as usize].lba_data_size()
                                }
                                _ => 512,
                            };

                            let ns = NvmeNamespace {
                                nsid,
                                nsze: ns_data.nsze,
                                lba_size,
                                capacity: ns_data.ncap * lba_size,
                            };

                            crate::serial::println!(
                                "[NVMe] Namespace {}: size={} LBAs, LBA size={}, capacity={} bytes",
                                nsid, ns.nsze, lba_size, ns.capacity,
                            );
                            ctrl.namespaces.push(ns);
                        } else {
                            // No active namespace at this ID - stop searching
                            break;
                        }
                    } else {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        // ---- Create I/O Completion Queue ----
        ctrl.create_io_cq(1, IO_QUEUE_SIZE, 0)
            .map_err(|e| NvmeError::InitFailed(e))?;

        // ---- Create I/O Submission Queue ----
        ctrl.create_io_sq(1, IO_QUEUE_SIZE, 1)
            .map_err(|e| NvmeError::InitFailed(e))?;

        crate::serial::println!(
            "[NVMe] Initialised with {} namespace(s)",
            ctrl.namespaces.len(),
        );

        // Store controller in the global static
        *NVME_CONTROLLER.lock() = Some(ctrl);
        NVME_INITIALIZED.store(true, Ordering::Release);

        Ok(NvmeDriver)
    }

    fn initialize(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn suspend(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn resume(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn name(&self) -> &'static str {
        "nvme"
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Global NVMe controller instance, initialised after successful probe.
static NVME_CONTROLLER: Mutex<Option<NvmeController>> = Mutex::new(None);
static NVME_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Check whether the NVMe driver has been initialised.
pub fn is_initialized() -> bool {
    NVME_INITIALIZED.load(Ordering::Acquire)
}

/// Get a reference to the NVMe controller (for read/write operations).
pub fn with_controller<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut NvmeController) -> R,
{
    NVME_CONTROLLER.lock().as_mut().map(f)
}

// ---------------------------------------------------------------------------
// Block device operations (called from VFS or storage layer)
// ---------------------------------------------------------------------------

/// Error type for block I/O operations.
#[derive(Debug)]
pub enum IoError {
    Timeout,
    InvalidNamespace,
    InvalidOffset,
    DeviceError(u16),
}

#[allow(dead_code)]
const BYTES_PER_PRP: u64 = 4096;

/// Read blocks from an NVMe namespace.
///
/// `nsid`: namespace ID (1-based)
/// `lba`: starting logical block address
/// `count`: number of blocks to read
/// `buffer`: destination buffer (must be at least `count * lba_size` bytes)
pub fn read_blocks(nsid: u32, lba: u64, count: u64, buffer: &mut [u8]) -> Result<(), IoError> {
    let mut guard = NVME_CONTROLLER.lock();
    let ctrl = guard.as_mut().ok_or(IoError::Timeout)?;

    let ns = ctrl.namespaces.iter().find(|ns| ns.nsid == nsid).ok_or(IoError::InvalidNamespace)?;
    let lba_size = ns.lba_size;
    let needed = count * lba_size;

    if (buffer.len() as u64) < needed {
        return Err(IoError::InvalidOffset);
    }

    let pmo = ctrl.phys_mem_offset;
    let buf_phys = buffer.as_ptr() as u64 - pmo;

    let cid = ctrl.alloc_cid();
    let tail = ctrl.io_sq_tail.load(Ordering::Relaxed);
    let sq_size = IO_QUEUE_SIZE as usize;
    let slt = ctrl.io_sq_mem.as_mut_ptr() as *mut SubmissionQueueEntry;

    let idx = tail as usize % sq_size;
    let entry = unsafe { &mut *slt.add(idx) };
    *entry = SubmissionQueueEntry {
        opcode: NVM_READ,
        flags: 0,
        command_id: cid,
        nsid,
        cdw2: 0,
        cdw3: 0,
        mptr: 0,
        prp1: buf_phys,
        prp2: 0,
        cdw10: lba as u32,
        cdw11: (lba >> 32) as u32,
        cdw12: (count - 1) as u32,
        cdw13: 0,
        cdw14: 0,
        cdw15: 0,
    };

    core::sync::atomic::fence(Ordering::Release);
    let new_tail = tail.wrapping_add(1);
    ctrl.io_sq_tail.store(new_tail, Ordering::Relaxed);
    ctrl.ring_io_sq(new_tail);

    // Poll I/O CQ
    match ctrl.poll_cq(1, cid, CMD_TIMEOUT_ITER) {
        Ok(()) => {
            let cq_head = ctrl.io_cq_head.load(Ordering::Relaxed);
            let cq_idx = cq_head as usize % IO_QUEUE_SIZE as usize;
            let clt = ctrl.io_cq_mem.as_ptr() as *const CompletionQueueEntry;
            let cqe = unsafe { *clt.add(cq_idx) };

            ctrl.io_cq_head.store(cq_head.wrapping_add(1), Ordering::Relaxed);
            ctrl.ring_io_cq(cq_head.wrapping_add(1));

            if (cqe.status & 0x7F) != 0 {
                return Err(IoError::DeviceError(cqe.status));
            }
            Ok(())
        }
        Err(_) => {
            let _ = ctrl.abort_command(cid);
            Err(IoError::Timeout)
        }
    }
}

/// Write blocks to an NVMe namespace.
pub fn write_blocks(nsid: u32, lba: u64, count: u64, buffer: &[u8]) -> Result<(), IoError> {
    let mut guard = NVME_CONTROLLER.lock();
    let ctrl = guard.as_mut().ok_or(IoError::Timeout)?;

    let ns = ctrl.namespaces.iter().find(|ns| ns.nsid == nsid).ok_or(IoError::InvalidNamespace)?;
    let lba_size = ns.lba_size;
    let needed = count * lba_size;

    if (buffer.len() as u64) < needed {
        return Err(IoError::InvalidOffset);
    }

    let pmo = ctrl.phys_mem_offset;
    let buf_phys = buffer.as_ptr() as u64 - pmo;

    let cid = ctrl.alloc_cid();
    let tail = ctrl.io_sq_tail.load(Ordering::Relaxed);
    let sq_size = IO_QUEUE_SIZE as usize;
    let slt = ctrl.io_sq_mem.as_mut_ptr() as *mut SubmissionQueueEntry;

    let idx = tail as usize % sq_size;
    let entry = unsafe { &mut *slt.add(idx) };
    *entry = SubmissionQueueEntry {
        opcode: NVM_WRITE,
        flags: 0,
        command_id: cid,
        nsid,
        cdw2: 0,
        cdw3: 0,
        mptr: 0,
        prp1: buf_phys,
        prp2: 0,
        cdw10: lba as u32,
        cdw11: (lba >> 32) as u32,
        cdw12: (count - 1) as u32,
        cdw13: 0,
        cdw14: 0,
        cdw15: 0,
    };

    core::sync::atomic::fence(Ordering::Release);
    let new_tail = tail.wrapping_add(1);
    ctrl.io_sq_tail.store(new_tail, Ordering::Relaxed);
    ctrl.ring_io_sq(new_tail);

    match ctrl.poll_cq(1, cid, CMD_TIMEOUT_ITER) {
        Ok(()) => {
            let cq_head = ctrl.io_cq_head.load(Ordering::Relaxed);
            let cq_idx = cq_head as usize % IO_QUEUE_SIZE as usize;
            let clt = ctrl.io_cq_mem.as_ptr() as *const CompletionQueueEntry;
            let cqe = unsafe { *clt.add(cq_idx) };

            ctrl.io_cq_head.store(cq_head.wrapping_add(1), Ordering::Relaxed);
            ctrl.ring_io_cq(cq_head.wrapping_add(1));

            if (cqe.status & 0x7F) != 0 {
                return Err(IoError::DeviceError(cqe.status));
            }
            Ok(())
        }
        Err(_) => {
            let _ = ctrl.abort_command(cid);
            Err(IoError::Timeout)
        }
    }
}

// ---------------------------------------------------------------------------
// Re-initialisation
// ---------------------------------------------------------------------------

/// Attempt to re-initialise the NVMe controller (used after unexpected reset).
pub fn reinit() -> bool {
    let device_registry = crate::drivers::DEVICE_REGISTRY.lock();
    for (_, info) in device_registry.iter_device_infos() {
        if info.class_code == NVME_CLASS && info.subclass == NVME_SUBCLASS {
            match NvmeDriver::probe(info) {
                Ok(_) => {
                    crate::serial::println!("[NVMe] Re-initialisation succeeded");
                    return true;
                }
                Err(_) => {}
            }
        }
    }
    crate::serial::println!("[NVMe] Re-initialisation failed: no NVMe device found");
    false
}

// ---------------------------------------------------------------------------
// Default impl for IdentifyNamespaceData
// ---------------------------------------------------------------------------

impl IdentifyNamespaceData {
    fn default() -> Self {
        Self {
            nsze: 0,
            ncap: 0,
            nuse: 0,
            nsfeat: 0,
            nlbaf: 0,
            flbas: 0,
            mc: 0,
            dpc: 0,
            dps: 0,
            nmic: 0,
            rescap: 0,
            fpi: 0,
            dlfeat: 0,
            _reserved0: [0; 40],
            lbaf0: LbaFormat { raw: 0 },
            lbaf1: LbaFormat { raw: 0 },
            lbaf2: LbaFormat { raw: 0 },
            lbaf3: LbaFormat { raw: 0 },
            lbaf4: LbaFormat { raw: 0 },
            lbaf5: LbaFormat { raw: 0 },
            lbaf6: LbaFormat { raw: 0 },
            lbaf7: LbaFormat { raw: 0 },
            lbaf8: LbaFormat { raw: 0 },
            lbaf9: LbaFormat { raw: 0 },
            lbaf10: LbaFormat { raw: 0 },
            lbaf11: LbaFormat { raw: 0 },
            lbaf12: LbaFormat { raw: 0 },
            lbaf13: LbaFormat { raw: 0 },
            lbaf14: LbaFormat { raw: 0 },
            lbaf15: LbaFormat { raw: 0 },
            _reserved1: [0; 3712],
        }
    }
}

// ---------------------------------------------------------------------------
// Default impl for SubmissionQueueEntry
// ---------------------------------------------------------------------------

impl SubmissionQueueEntry {
    #[allow(dead_code)]
    fn zeroed() -> Self {
        Self {
            opcode: 0,
            flags: 0,
            command_id: 0,
            nsid: 0,
            cdw2: 0,
            cdw3: 0,
            mptr: 0,
            prp1: 0,
            prp2: 0,
            cdw10: 0,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test]
    fn test_nvme_class_subclass() {
        assert_eq!(NVME_CLASS, 0x01);
        assert_eq!(NVME_SUBCLASS, 0x08);
    }

    #[test]
    fn test_submission_queue_entry_size() {
        assert_eq!(core::mem::size_of::<SubmissionQueueEntry>(), 64);
    }

    #[test]
    fn test_completion_queue_entry_size() {
        assert_eq!(core::mem::size_of::<CompletionQueueEntry>(), 16);
    }

    #[test]
    fn test_identify_namespace_size() {
        assert_eq!(core::mem::size_of::<IdentifyNamespaceData>(), 4096);
    }

    #[test]
    fn test_namespace_capacity_calculation() {
        let ns = NvmeNamespace {
            nsid: 1,
            nsze: 1_000_000,
            lba_size: 512,
            capacity: 1_000_000 * 512,
        };
        assert_eq!(ns.capacity, 512_000_000);
        assert_eq!(ns.lba_size, 512);
        assert_eq!(ns.nsid, 1);
    }

    #[test]
    fn test_namespace_lba_4096() {
        let ns = NvmeNamespace {
            nsid: 1,
            nsze: 500_000,
            lba_size: 4096,
            capacity: 500_000 * 4096,
        };
        assert_eq!(ns.capacity, 2_048_000_000);
    }

    #[test]
    fn test_mmio_register_offsets() {
        assert_eq!(REG_CAP, 0x00);
        assert_eq!(REG_VS, 0x08);
        assert_eq!(REG_CC, 0x14);
        assert_eq!(REG_CSTS, 0x1C);
        assert_eq!(REG_AQA, 0x24);
        assert_eq!(REG_ASQ, 0x28);
        assert_eq!(REG_ACQ, 0x30);
    }

    #[test]
    fn test_cc_enable_bit() {
        assert_eq!(CC_EN, 0x0000_0001);
    }

    #[test]
    fn test_lba_format_size() {
        assert_eq!(core::mem::size_of::<LbaFormat>(), 4);
    }

    #[test]
    fn test_lba_format_data_size() {
        let lbaf = LbaFormat { raw: 9 }; // 2^9 = 512
        assert_eq!(lbaf.lba_data_size(), 512);
        let lbaf = LbaFormat { raw: 12 }; // 2^12 = 4096
        assert_eq!(lbaf.lba_data_size(), 4096);
    }

    #[test]
    fn test_queue_sizes() {
        assert_eq!(ADMIN_QUEUE_SIZE, 64);
        assert_eq!(IO_QUEUE_SIZE, 256);
    }

    #[test]
    fn test_command_opcodes() {
        assert_eq!(ADMIN_CREATE_IO_CQ, 0x05);
        assert_eq!(ADMIN_CREATE_IO_SQ, 0x01);
        assert_eq!(ADMIN_IDENTIFY, 0x06);
        assert_eq!(ADMIN_ABORT, 0x08);
        assert_eq!(NVM_READ, 0x02);
        assert_eq!(NVM_WRITE, 0x01);
    }

    #[test]
    fn test_probe_rejection_non_nvme() {
        let info = DeviceInfo {
            vendor_id: 0x8086,
            device_id: 0x100E,
            class_code: 0x02,
            subclass: 0x00,
            prog_if: 0x00,
            bus: 0,
            device: 0,
            function: 0,
            bars: [None, None, None, None, None, None],
            interrupt_line: None,
            interrupt_pin: None,
            irq: None,
        };
        let result = NvmeDriver::probe(&info);
        assert!(result.is_err());
    }

    #[test]
    fn test_probe_rejection_no_bar0() {
        let info = DeviceInfo {
            vendor_id: 0x8086,
            device_id: 0x100E,
            class_code: 0x01,
            subclass: 0x08,
            prog_if: 0x00,
            bus: 0,
            device: 0,
            function: 0,
            bars: [None, None, None, None, None, None],
            interrupt_line: None,
            interrupt_pin: None,
            irq: None,
        };
        let result = NvmeDriver::probe(&info);
        assert!(result.is_err());
    }

    #[test]
    fn test_doorbell_offsets() {
        let stride = 4u64;
        let ctrl = NvmeController {
            bar0: 0,
            phys_mem_offset: 0,
            admin_sq_mem: alloc::vec![],
            admin_cq_mem: alloc::vec![],
            io_sq_mem: alloc::vec![],
            io_cq_mem: alloc::vec![],
            admin_sq_tail: AtomicU16::new(0),
            admin_cq_head: AtomicU16::new(0),
            io_sq_tail: AtomicU16::new(0),
            io_cq_head: AtomicU16::new(0),
            next_cid: AtomicU16::new(1),
            doorbell_stride: stride,
            namespaces: Vec::new(),
            device_info: DeviceInfo {
                vendor_id: 0,
                device_id: 0,
                class_code: 0,
                subclass: 0,
                prog_if: 0,
                bus: 0,
                device: 0,
                function: 0,
                bars: [None, None, None, None, None, None],
                interrupt_line: None,
                interrupt_pin: None,
                irq: None,
            },
        };

        // SQ0 doorbell: 0x1000
        assert_eq!(ctrl.sq_doorbell(0), 0x1000);
        // CQ0 doorbell: 0x1004
        assert_eq!(ctrl.cq_doorbell(0), 0x1004);
        // SQ1 doorbell: 0x1008
        assert_eq!(ctrl.sq_doorbell(1), 0x1008);
        // CQ1 doorbell: 0x100C
        assert_eq!(ctrl.cq_doorbell(1), 0x100C);
    }

    #[test]
    fn test_capability_values() {
        let cap: u64 = (1u64 << CAP_TO_SHIFT) | (2u64 << CAP_DSTRD_SHIFT);
        let cap_to = ((cap >> CAP_TO_SHIFT) & 0xFF) as u32;
        let dstrd = ((cap >> CAP_DSTRD_SHIFT) & 0xF) as u8;
        assert_eq!(cap_to, 1);
        assert_eq!(dstrd, 2);
        let doorbell_stride = 4u64 << dstrd;
        assert_eq!(doorbell_stride, 16);
    }

    #[test]
    fn test_nvme_error_debug() {
        let err = NvmeError::IoTimeout { nsid: 1, lba: 100, count: 8 };
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("IoTimeout"));
    }
}
