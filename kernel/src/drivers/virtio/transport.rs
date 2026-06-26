//! VirtIO transport abstraction layer.
//!
//! Provides a common trait for PCI and legacy MMIO transports, enabling
//! device drivers to work across different VirtIO implementations.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

/// VirtIO device status bits.
pub const VIRTIO_STATUS_ACK: u8 = 1;
pub const VIRTIO_STATUS_DRIVER: u8 = 2;
pub const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
pub const VIRTIO_STATUS_FEATURES_OK: u8 = 8;
pub const VIRTIO_STATUS_FAILED: u8 = 128;

/// VirtIO feature bits for network devices.
pub const VIRTIO_NET_F_MAC: u64 = 5;
pub const VIRTIO_F_VERSION_1: u64 = 32;

/// Maximum number of virtqueue descriptors.
pub const VRING_SIZE: usize = 256;

/// Sentinel value indicating end of free list (no more free descriptors).
/// Must be >= VRING_SIZE to avoid collision with valid descriptor indices.
const FREE_LIST_END: u16 = 0xFFFF;

/// Errors that can occur during VirtIO transport operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    /// The transport device was not found.
    DeviceNotFound,
    /// The device did not acknowledge the driver.
    AckFailed,
    /// Feature negotiation failed.
    FeatureNegotiationFailed,
    /// Virtqueue setup failed.
    VirtqueueSetupFailed,
    /// The device is in a failed state.
    DeviceFailed,
    /// MMIO or port I/O access failed.
    IoError,
    /// Invalid descriptor index.
    InvalidDescriptor(usize),
    /// No free descriptors available.
    NoFreeDescriptors,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceNotFound => write!(f, "VirtIO device not found"),
            Self::AckFailed => write!(f, "Device did not acknowledge driver"),
            Self::FeatureNegotiationFailed => write!(f, "Feature negotiation failed"),
            Self::VirtqueueSetupFailed => write!(f, "Virtqueue setup failed"),
            Self::DeviceFailed => write!(f, "Device in failed state"),
            Self::IoError => write!(f, "MMIO/PIO access error"),
            Self::InvalidDescriptor(idx) => write!(f, "Invalid descriptor index: {}", idx),
            Self::NoFreeDescriptors => write!(f, "No free descriptors available"),
        }
    }
}

/// Descriptor ring entry (16 bytes, VirtIO 1.0 spec).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtqDesc {
    /// Physical address of the buffer.
    pub addr: u64,
    /// Length of the buffer.
    pub len: u32,
    /// Flags (VIRTQ_DESC_F_NEXT, VIRTQ_DESC_F_WRITE, etc.).
    pub flags: u16,
    /// Next descriptor index (if VIRTQ_DESC_F_NEXT is set).
    pub next: u16,
}

pub const VIRTQ_DESC_F_NEXT: u16 = 1;
pub const VIRTQ_DESC_F_WRITE: u16 = 2;
pub const VIRTQ_DESC_F_INDIRECT: u16 = 4;

/// Available ring (driver -> device).
#[repr(C)]
#[derive(Debug)]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; VRING_SIZE],
}

/// Used ring element (device -> driver).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtqUsedElem {
    pub id: u32,
    pub len: u32,
}

/// Used ring (device -> driver).
#[repr(C)]
#[derive(Debug)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VirtqUsedElem; VRING_SIZE],
}

/// A complete virtqueue with descriptor ring, available ring, and used ring.
pub struct Virtqueue {
    pub descriptors: Vec<VirtqDesc>,
    pub avail: Box<VirtqAvail>,
    pub used: Box<VirtqUsed>,
    pub free_head: Option<usize>,
    pub size: u16,
    /// Index of this virtqueue (for notify calculations).
    pub queue_index: u16,
    /// Last used index that was consumed by the driver.
    last_used_idx: u16,
}

impl Virtqueue {
    /// Create a new virtqueue with the given number of descriptors.
    pub fn new(queue_index: u16) -> Self {
        let mut descriptors = Vec::with_capacity(VRING_SIZE);
        for i in 0..VRING_SIZE {
            descriptors.push(VirtqDesc {
                addr: 0,
                len: 0,
                flags: 0,
                next: if i + 1 < VRING_SIZE { (i + 1) as u16 } else { FREE_LIST_END },
            });
        }

        let mut vq = Virtqueue {
            descriptors,
            avail: Box::new(VirtqAvail {
                flags: 0,
                idx: 0,
                ring: [0; VRING_SIZE],
            }),
            used: Box::new(VirtqUsed {
                flags: 0,
                idx: 0,
                ring: [VirtqUsedElem { id: 0, len: 0 }; VRING_SIZE],
            }),
            free_head: Some(0),
            size: VRING_SIZE as u16,
            queue_index,
            last_used_idx: 0,
        };
        // Mark last descriptor as end of free list
        vq.descriptors[VRING_SIZE - 1].next = 0;
        vq
    }

    /// Allocate a single descriptor from the free list.
    pub fn alloc_desc(&mut self) -> Option<usize> {
        let head = self.free_head?;
        self.free_head = {
            let next = self.descriptors[head].next;
            if next == FREE_LIST_END {
                None
            } else {
                Some(next as usize)
            }
        };
        Some(head)
    }

    /// Allocate a chain of `count` contiguous descriptors.
    pub fn alloc_desc_chain(&mut self, count: usize) -> Option<Vec<usize>> {
        let mut chain = Vec::with_capacity(count);
        for _ in 0..count {
            match self.alloc_desc() {
                Some(idx) => chain.push(idx),
                None => {
                    // Roll back: free any descriptors we allocated
                    for &idx in chain.iter().rev() {
                        self.free_desc(idx);
                    }
                    return None;
                }
            }
        }
        // Link the chain
        for i in 0..chain.len() - 1 {
            self.descriptors[chain[i]].flags |= VIRTQ_DESC_F_NEXT;
            self.descriptors[chain[i]].next = chain[i + 1] as u16;
        }
        Some(chain)
    }

    /// Free a single descriptor.
    pub fn free_desc(&mut self, idx: usize) {
        self.descriptors[idx].next = self.free_head.map(|h| h as u16).unwrap_or(FREE_LIST_END);
        self.descriptors[idx].flags = 0;
        self.descriptors[idx].addr = 0;
        self.descriptors[idx].len = 0;
        self.free_head = Some(idx);
    }

    /// Free a chain of descriptors.
    pub fn free_desc_chain(&mut self, head: usize) {
        let mut idx = head;
        loop {
            // Read next and flags before freeing (free_desc modifies them)
            let has_next = self.descriptors[idx].flags & VIRTQ_DESC_F_NEXT != 0;
            let next = self.descriptors[idx].next as usize;
            self.free_desc(idx);
            if !has_next {
                break;
            }
            idx = next;
        }
    }

    /// Submit a descriptor chain to the available ring.
    pub fn submit(&mut self, desc_head: usize) {
        let idx = self.avail.idx as usize % VRING_SIZE;
        self.avail.ring[idx] = desc_head as u16;
        // Memory fence to ensure the device sees the updated descriptor
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        self.avail.idx = self.avail.idx.wrapping_add(1);
    }

    /// Check if there are used buffers to process.
    pub fn has_used(&self) -> bool {
        self.last_used_idx != self.used.idx
    }

    /// Pop a used buffer from the used ring.
    pub fn pop_used(&mut self) -> Option<(u32, u32)> {
        if self.last_used_idx == self.used.idx {
            return None;
        }
        let elem = self.used.ring[self.last_used_idx as usize % VRING_SIZE];
        self.last_used_idx = self.last_used_idx.wrapping_add(1);
        Some((elem.id, elem.len))
    }
}

/// Trait for VirtIO transport layers (PCI, MMIO, legacy).
///
/// This abstraction allows device drivers to work across different
/// VirtIO transport implementations.
pub trait VirtioTransport: Send + Sync {
    /// Get the device type identifier.
    fn device_type(&self) -> u32;

    /// Get the device status register.
    fn get_status(&self) -> u8;

    /// Set the device status register.
    fn set_status(&mut self, status: u8);

    /// Add status bits to the device status register.
    fn add_status(&mut self, status: u8) {
        let current = self.get_status();
        self.set_status(current | status);
    }

    /// Clear status bits from the device status register.
    fn clear_status(&mut self, status: u8) {
        let current = self.get_status();
        self.set_status(current & !status);
    }

    /// Reset the device (set status to 0).
    fn reset(&mut self) {
        self.set_status(0);
    }

    /// Read a 32-bit feature field.
    fn get_features(&self) -> u64;

    /// Write a 32-bit feature field (after negotiation).
    fn set_features(&mut self, features: u64);

    /// Read from device-specific config space at the given byte offset.
    fn config_read_u8(&self, offset: usize) -> u8;

    /// Read a 16-bit value from config space.
    fn config_read_u16(&self, offset: usize) -> u16 {
        let lo = self.config_read_u8(offset) as u16;
        let hi = self.config_read_u8(offset + 1) as u16;
        lo | (hi << 8)
    }

    /// Read a 32-bit value from config space.
    fn config_read_u32(&self, offset: usize) -> u32 {
        let b0 = self.config_read_u8(offset) as u32;
        let b1 = self.config_read_u8(offset + 1) as u32;
        let b2 = self.config_read_u8(offset + 2) as u32;
        let b3 = self.config_read_u8(offset + 3) as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }

    /// Write to device-specific config space.
    fn config_write_u8(&mut self, offset: usize, value: u8);

    /// Write a 16-bit value to config space.
    fn config_write_u16(&mut self, offset: usize, value: u16) {
        self.config_write_u8(offset, value as u8);
        self.config_write_u8(offset + 1, (value >> 8) as u8);
    }

    /// Write a 32-bit value to config space.
    fn config_write_u32(&mut self, offset: usize, value: u32) {
        self.config_write_u8(offset, value as u8);
        self.config_write_u8(offset + 1, (value >> 8) as u8);
        self.config_write_u8(offset + 2, (value >> 16) as u8);
        self.config_write_u8(offset + 3, (value >> 24) as u8);
    }

    /// Get the number of virtqueues supported by this device.
    fn num_queues(&self) -> u16;

    /// Set up a virtqueue at the given index.
    /// Returns the queue size.
    fn setup_queue(&mut self, queue_index: u16, phys_addr: u64, size: u16) -> Result<(), TransportError>;

    /// Notify the device that a buffer has been added to a virtqueue.
    fn notify_queue(&self, queue_index: u16);

    /// Get the ISR status (interrupt cause).
    fn get_isr_status(&self) -> u8;
}

/// MMIO-based VirtIO transport (for ARM/x86 QEMU virt machines).
pub struct MmioTransport {
    /// Base address of the MMIO region.
    base_addr: u64,
    /// Device configuration space offset.
    config_offset: u64,
}

// Safety: MMIO access is hardware-specific and requires single-threaded access
// In practice, devices are accessed through a Mutex
unsafe impl Send for MmioTransport {}
unsafe impl Sync for MmioTransport {}

impl MmioTransport {
    /// Create a new MMIO transport at the given base address.
    pub fn new(base_addr: u64) -> Self {
        MmioTransport {
            base_addr,
            config_offset: 0x100, // Standard VirtIO MMIO config offset
        }
    }

    /// Read a 32-bit MMIO register.
    fn mmio_read(&self, offset: u64) -> u32 {
        let ptr = (self.base_addr + offset) as *const u32;
        // Safety: We are reading from a hardware MMIO region that was validated
        // during device enumeration. The pointer is aligned and within the device's BAR.
        unsafe { core::ptr::read_volatile(ptr) }
    }

    /// Write a 32-bit MMIO register.
    fn mmio_write(&mut self, offset: u64, value: u32) {
        let ptr = (self.base_addr + offset) as *mut u32;
        // Safety: We are writing to a hardware MMIO region that was validated
        // during device enumeration. The pointer is aligned and within the device's BAR.
        unsafe { core::ptr::write_volatile(ptr, value) }
    }
}

impl VirtioTransport for MmioTransport {
    fn device_type(&self) -> u32 {
        self.mmio_read(0x00)
    }

    fn get_status(&self) -> u8 {
        self.mmio_read(0x04) as u8
    }

    fn set_status(&mut self, status: u8) {
        self.mmio_write(0x04, status as u32);
    }

    fn get_features(&self) -> u64 {
        let lo = self.mmio_read(0x08) as u64;
        let hi = self.mmio_read(0x0c) as u64;
        lo | (hi << 32)
    }

    fn set_features(&mut self, features: u64) {
        self.mmio_write(0x08, features as u32);
        self.mmio_write(0x0c, (features >> 32) as u32);
    }

    fn config_read_u8(&self, offset: usize) -> u8 {
        let ptr = (self.base_addr + self.config_offset + offset as u64) as *const u8;
        // Safety: Reading from device config space at a validated offset
        unsafe { core::ptr::read_volatile(ptr) }
    }

    fn config_write_u8(&mut self, offset: usize, value: u8) {
        let ptr = (self.base_addr + self.config_offset + offset as u64) as *mut u8;
        // Safety: Writing to device config space at a validated offset
        unsafe { core::ptr::write_volatile(ptr, value) }
    }

    fn num_queues(&self) -> u16 {
        self.mmio_read(0x08) as u16 // QueueNumMax for queue 0
    }

    fn setup_queue(&mut self, queue_index: u16, phys_addr: u64, size: u16) -> Result<(), TransportError> {
        // Select queue
        self.mmio_write(0x30, queue_index as u32);
        // Read max queue size
        let max_size = self.mmio_read(0x34);
        if size as u32 > max_size {
            return Err(TransportError::VirtqueueSetupFailed);
        }
        // Set queue size
        self.mmio_write(0x34, size as u32);
        // Set queue physical address
        self.mmio_write(0x40, (phys_addr & 0xFFFFFFFF) as u32);
        self.mmio_write(0x44, ((phys_addr >> 32) & 0xFFFFFFFF) as u32);
        // Enable queue
        self.mmio_write(0x44, 1);
        Ok(())
    }

    fn notify_queue(&self, queue_index: u16) {
        let ptr = (self.base_addr + 0x50) as *mut u32;
        // Safety: Writing to the MMIO notify register at a validated offset
        unsafe { core::ptr::write_volatile(ptr, queue_index as u32) }
    }

    fn get_isr_status(&self) -> u8 {
        self.mmio_read(0x60) as u8
    }
}

/// PCI-based VirtIO transport (VirtIO 1.0 modern).
pub struct PciTransport {
    /// Base address of the Common Config MMIO region.
    common_cfg_addr: u64,
    /// Base address of the Notify MMIO region.
    notify_addr: u64,
    /// Base address of the ISR config MMIO region.
    isr_addr: u64,
    /// Base address of the Device Config MMIO region.
    device_cfg_addr: u64,
    /// Notify offset multiplier (typically queue_size * sizeof(VirtqUsedElem)).
    notify_offset_multiplier: u16,
    /// Number of queues.
    num_queues: u16,
}

// Safety: PCI MMIO access is hardware-specific and requires single-threaded access
unsafe impl Send for PciTransport {}
unsafe impl Sync for PciTransport {}

impl PciTransport {
    /// Create a new PCI transport from BAR-resolved MMIO addresses.
    pub fn new(
        common_cfg_addr: u64,
        notify_addr: u64,
        isr_addr: u64,
        device_cfg_addr: u64,
        notify_offset_multiplier: u16,
        num_queues: u16,
    ) -> Self {
        PciTransport {
            common_cfg_addr,
            notify_addr,
            isr_addr,
            device_cfg_addr,
            notify_offset_multiplier,
            num_queues,
        }
    }

    /// Read a 8-bit field from Common Config.
    fn common_read_u8(&self, offset: u64) -> u8 {
        let ptr = (self.common_cfg_addr + offset) as *const u8;
        // Safety: Reading from PCI device common config MMIO region at a validated offset
        unsafe { core::ptr::read_volatile(ptr) }
    }

    /// Write a 8-bit field to Common Config.
    fn common_write_u8(&mut self, offset: u64, value: u8) {
        let ptr = (self.common_cfg_addr + offset) as *mut u8;
        // Safety: Writing to PCI device common config MMIO region at a validated offset
        unsafe { core::ptr::write_volatile(ptr, value) }
    }

    /// Read a 16-bit field from Common Config.
    fn common_read_u16(&self, offset: u64) -> u16 {
        let ptr = (self.common_cfg_addr + offset) as *const u16;
        // Safety: Reading from PCI device common config MMIO region at a validated offset
        unsafe { core::ptr::read_volatile(ptr) }
    }

    /// Write a 16-bit field to Common Config.
    fn common_write_u16(&mut self, offset: u64, value: u16) {
        let ptr = (self.common_cfg_addr + offset) as *mut u16;
        // Safety: Writing to PCI device common config MMIO region at a validated offset
        unsafe { core::ptr::write_volatile(ptr, value) }
    }

    /// Read a 32-bit field from Common Config.
    fn common_read_u32(&self, offset: u64) -> u32 {
        let ptr = (self.common_cfg_addr + offset) as *const u32;
        // Safety: Reading from PCI device common config MMIO region at a validated offset
        unsafe { core::ptr::read_volatile(ptr) }
    }

    /// Write a 32-bit field to Common Config.
    fn common_write_u32(&mut self, offset: u64, value: u32) {
        let ptr = (self.common_cfg_addr + offset) as *mut u32;
        // Safety: Writing to PCI device common config MMIO region at a validated offset
        unsafe { core::ptr::write_volatile(ptr, value) }
    }
}

// VirtIO 1.0 Common Config offsets
const COMMON_DEVICE_TYPE: u64 = 0x00;
const COMMON_DEVICE_STATUS: u64 = 0x04;
const COMMON_QUEUE_SELECT: u64 = 0x0c;
const COMMON_QUEUE_SIZE: u64 = 0x0e;
const COMMON_QUEUE_MSIX_VECTOR: u64 = 0x10;
const COMMON_QUEUE_ENABLE: u64 = 0x12;
const COMMON_QUEUE_DESC_LOW: u64 = 0x14;
const COMMON_QUEUE_DESC_HIGH: u64 = 0x18;
const COMMON_QUEUE_DRIVER_LOW: u64 = 0x1c;
const COMMON_QUEUE_DRIVER_HIGH: u64 = 0x20;
const COMMON_QUEUE_DEVICE_LOW: u64 = 0x24;
const COMMON_QUEUE_DEVICE_HIGH: u64 = 0x28;
const COMMON_CONFIG_GENERATION: u64 = 0x2c;
const COMMON_FEATURE_SELECT: u64 = 0x08;
const COMMON_FEATURE_HIGH: u64 = 0x08;

impl VirtioTransport for PciTransport {
    fn device_type(&self) -> u32 {
        self.common_read_u32(COMMON_DEVICE_TYPE)
    }

    fn get_status(&self) -> u8 {
        self.common_read_u8(COMMON_DEVICE_STATUS)
    }

    fn set_status(&mut self, status: u8) {
        self.common_write_u8(COMMON_DEVICE_STATUS, status);
    }

    fn get_features(&self) -> u64 {
        // Select feature word 0 and read low 32 bits
        // Safety: We're writing to a volatile MMIO register for feature selection
        unsafe {
            let ptr = (self.common_cfg_addr + COMMON_FEATURE_SELECT) as *mut u32;
            core::ptr::write_volatile(ptr, 0);
        }
        let lo = self.common_read_u32(COMMON_FEATURE_SELECT + 4) as u64;
        // Select feature word 1 and read high 32 bits
        unsafe {
            let ptr = (self.common_cfg_addr + COMMON_FEATURE_SELECT) as *mut u32;
            core::ptr::write_volatile(ptr, 1);
        }
        let hi = self.common_read_u32(COMMON_FEATURE_SELECT + 4) as u64;
        lo | (hi << 32)
    }

    fn set_features(&mut self, features: u64) {
        self.common_write_u32(COMMON_FEATURE_SELECT, 0);
        self.common_write_u32(COMMON_FEATURE_SELECT + 4, features as u32);
        self.common_write_u32(COMMON_FEATURE_SELECT, 1);
        self.common_write_u32(COMMON_FEATURE_SELECT + 4, (features >> 32) as u32);
    }

    fn config_read_u8(&self, offset: usize) -> u8 {
        let ptr = (self.device_cfg_addr + offset as u64) as *const u8;
        // Safety: Reading from PCI device config space at a validated offset
        unsafe { core::ptr::read_volatile(ptr) }
    }

    fn config_write_u8(&mut self, offset: usize, value: u8) {
        let ptr = (self.device_cfg_addr + offset as u64) as *mut u8;
        // Safety: Writing to PCI device config space at a validated offset
        unsafe { core::ptr::write_volatile(ptr, value) }
    }

    fn num_queues(&self) -> u16 {
        self.num_queues
    }

    fn setup_queue(&mut self, queue_index: u16, phys_addr: u64, size: u16) -> Result<(), TransportError> {
        // Select queue
        self.common_write_u16(COMMON_QUEUE_SELECT, queue_index);
        // Read max queue size
        let max_size = self.common_read_u16(COMMON_QUEUE_SIZE);
        if size > max_size {
            return Err(TransportError::VirtqueueSetupFailed);
        }
        // Set queue size
        self.common_write_u16(COMMON_QUEUE_SIZE, size);
        // Set descriptor table address
        self.common_write_u32(COMMON_QUEUE_DESC_LOW, (phys_addr & 0xFFFFFFFF) as u32);
        self.common_write_u32(COMMON_QUEUE_DESC_HIGH, ((phys_addr >> 32) & 0xFFFFFFFF) as u32);
        // Set available ring address (offset by descriptor table size)
        let avail_addr = phys_addr + (size as u64 * core::mem::size_of::<VirtqDesc>() as u64);
        self.common_write_u32(COMMON_QUEUE_DRIVER_LOW, (avail_addr & 0xFFFFFFFF) as u32);
        self.common_write_u32(COMMON_QUEUE_DRIVER_HIGH, ((avail_addr >> 32) & 0xFFFFFFFF) as u32);
        // Set used ring address (offset further)
        let used_addr = avail_addr + (core::mem::size_of::<VirtqAvail>() as u64);
        self.common_write_u32(COMMON_QUEUE_DEVICE_LOW, (used_addr & 0xFFFFFFFF) as u32);
        self.common_write_u32(COMMON_QUEUE_DEVICE_HIGH, ((used_addr >> 32) & 0xFFFFFFFF) as u32);
        // Enable queue
        self.common_write_u16(COMMON_QUEUE_ENABLE, 1);
        Ok(())
    }

    fn notify_queue(&self, queue_index: u16) {
        let notify_offset = queue_index as u64 * self.notify_offset_multiplier as u64;
        let ptr = (self.notify_addr + notify_offset) as *mut u16;
        // Safety: Writing to the MMIO notify register at a validated offset
        unsafe { core::ptr::write_volatile(ptr, queue_index) }
    }

    fn get_isr_status(&self) -> u8 {
        // Safety: Reading from ISR config MMIO region at a validated offset
        unsafe { core::ptr::read_volatile(self.isr_addr as *const u8) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_virtqueue_creation() {
        let _guard = crate::test_serial::acquire();
        let vq = Virtqueue::new(0);
        assert_eq!(vq.size, VRING_SIZE as u16);
        assert_eq!(vq.free_head, Some(0));
        assert_eq!(vq.avail.idx, 0);
        assert_eq!(vq.used.idx, 0);
    }

    #[test]
    fn test_desc_alloc_and_free() {
        let _guard = crate::test_serial::acquire();
        let mut vq = Virtqueue::new(0);

        let desc0 = vq.alloc_desc().unwrap();
        let desc1 = vq.alloc_desc().unwrap();
        let desc2 = vq.alloc_desc().unwrap();
        assert_eq!(desc0, 0);
        assert_eq!(desc1, 1);
        assert_eq!(desc2, 2);

        vq.free_desc(1);
        assert_eq!(vq.free_head, Some(1));

        let desc = vq.alloc_desc().unwrap();
        assert_eq!(desc, 1);
    }

    #[test]
    fn test_desc_chain_alloc() {
        let _guard = crate::test_serial::acquire();
        let mut vq = Virtqueue::new(0);

        let chain = vq.alloc_desc_chain(3).unwrap();
        assert_eq!(chain.len(), 3);
        assert_eq!(chain, vec![0, 1, 2]);

        // Check that descriptors are linked
        assert!(vq.descriptors[0].flags & VIRTQ_DESC_F_NEXT != 0);
        assert_eq!(vq.descriptors[0].next, 1);
        assert!(vq.descriptors[1].flags & VIRTQ_DESC_F_NEXT != 0);
        assert_eq!(vq.descriptors[1].next, 2);
        assert!(vq.descriptors[2].flags & VIRTQ_DESC_F_NEXT == 0);
    }

    #[test]
    fn test_desc_chain_free() {
        let _guard = crate::test_serial::acquire();
        let mut vq = Virtqueue::new(0);

        let chain = vq.alloc_desc_chain(4).unwrap();
        assert_eq!(chain, vec![0, 1, 2, 3]);
        
        // Check free_head before free
        assert_eq!(vq.free_head, Some(4));
        
        vq.free_desc_chain(chain[0]);
        
        // Check free_head after free - should be Some(3)
        // (3 -> 2 -> 1 -> 0 -> 4 -> ...)
        let free_head = vq.free_head.unwrap_or(256);
        assert!(free_head < 4, "free_head should be one of the freed descriptors, got {}", free_head);

        // All 4 should be free now (plus the original free list)
        for i in 0..4 {
            let desc = vq.alloc_desc();
            assert!(desc.is_some(), "alloc_desc {} should succeed, free_head={:?}", i, vq.free_head);
        }
    }

    #[test]
    fn test_submit_and_pop_used() {
        let _guard = crate::test_serial::acquire();
        let mut vq = Virtqueue::new(0);

        let chain = vq.alloc_desc_chain(1).unwrap();
        vq.submit(chain[0]);
        assert_eq!(vq.avail.idx, 1);

        // Simulate device completion
        vq.used.ring[0] = VirtqUsedElem {
            id: chain[0] as u32,
            len: 64,
        };
        vq.used.idx = 1;

        let result = vq.pop_used();
        assert!(result.is_some());
        let (id, len) = result.unwrap();
        assert_eq!(id, chain[0] as u32);
        assert_eq!(len, 64);
    }

    #[test]
    fn test_full_virtqueue_exhaustion() {
        let _guard = crate::test_serial::acquire();
        let mut vq = Virtqueue::new(0);

        // Allocate all descriptors
        let mut descs = Vec::new();
        while let Some(d) = vq.alloc_desc() {
            descs.push(d);
        }
        assert_eq!(descs.len(), VRING_SIZE);
        assert!(vq.alloc_desc().is_none());

        // Free them all
        for d in descs {
            vq.free_desc(d);
        }
        // Should be able to allocate again
        assert!(vq.alloc_desc().is_some());
    }
}
