use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use crate::boot::get_phys_mem_offset;
use crate::drivers::framework::{Bar, DeviceDriver, DeviceInfo};

const VIRTIO_VENDOR: u16 = 0x1AF4;
const VIRTIO_NET_TRANSITIONAL: u16 = 0x1000;
const VIRTIO_NET_MODERN: u16 = 0x1041;

const CAP_VENDOR_SPECIFIC: u8 = 0x09;
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;

const STATUS_ACK: u8 = 0x01;
const STATUS_DRIVER: u8 = 0x02;
const STATUS_DRIVER_OK: u8 = 0x04;
const STATUS_FEATURES_OK: u8 = 0x08;
const STATUS_FAILED: u8 = 0x80;

const VIRTIO_NET_F_MAC: u64 = 1 << 5;
const VIRTIO_F_VERSION_1: u64 = 1 << 32;

const COMMON_DEVICE_FEATURES: u16 = 0x00;
const COMMON_DEVICE_FEATURE_SEL: u16 = 0x04;
const COMMON_DRIVER_FEATURES: u16 = 0x08;
const COMMON_DRIVER_FEATURE_SEL: u16 = 0x0C;
const COMMON_DEVICE_STATUS: u16 = 0x12;
const COMMON_QUEUE_SEL: u16 = 0x14;
const COMMON_QUEUE_SIZE: u16 = 0x16;
const COMMON_QUEUE_ENABLE: u16 = 0x1A;
const COMMON_QUEUE_NOTIFY_OFF: u16 = 0x1C;
const COMMON_QUEUE_DESC: u16 = 0x20;
const COMMON_QUEUE_DRIVER: u16 = 0x28;
const COMMON_QUEUE_DEVICE: u16 = 0x30;

const QUEUE_SIZE: u16 = 256;

const DEVICE_CFG_MAC_OFFSET: u16 = 0x00;

fn mmio_read_u8(base: u64, off: u16) -> u8 {
    unsafe { read_volatile((base + off as u64) as *const u8) }
}

fn mmio_read_u16(base: u64, off: u16) -> u16 {
    unsafe { read_volatile((base + off as u64) as *const u16) }
}

fn mmio_read_u32(base: u64, off: u16) -> u32 {
    unsafe { read_volatile((base + off as u64) as *const u32) }
}

fn mmio_write_u8(base: u64, off: u16, val: u8) {
    unsafe { write_volatile((base + off as u64) as *mut u8, val); }
}

fn mmio_write_u16(base: u64, off: u16, val: u16) {
    unsafe { write_volatile((base + off as u64) as *mut u16, val); }
}

fn mmio_write_u32(base: u64, off: u16, val: u32) {
    unsafe { write_volatile((base + off as u64) as *mut u32, val); }
}

fn mmio_write_u64(base: u64, off: u16, val: u64) {
    unsafe { write_volatile((base + off as u64) as *mut u64, val); }
}

fn pci_cfg_read_u8(bus: u8, dev: u8, func: u8, off: u16) -> u8 {
    let addr = 0x8000_0000u32
        | (bus as u32) << 16
        | (dev as u32) << 11
        | (func as u32) << 8
        | (off as u32 & 0xFC);
    unsafe {
        let mut ca: x86_64::instructions::port::Port<u32> =
            x86_64::instructions::port::Port::new(0xCF8);
        let mut cd: x86_64::instructions::port::Port<u32> =
            x86_64::instructions::port::Port::new(0xCFC);
        ca.write(addr);
        ((cd.read() >> ((off & 3) * 8)) & 0xFF) as u8
    }
}

fn pci_cfg_read_u32(bus: u8, dev: u8, func: u8, off: u16) -> u32 {
    let addr = 0x8000_0000u32
        | (bus as u32) << 16
        | (dev as u32) << 11
        | (func as u32) << 8
        | (off as u32 & 0xFC);
    unsafe {
        let mut ca: x86_64::instructions::port::Port<u32> =
            x86_64::instructions::port::Port::new(0xCF8);
        let mut cd: x86_64::instructions::port::Port<u32> =
            x86_64::instructions::port::Port::new(0xCFC);
        ca.write(addr);
        cd.read()
    }
}

#[derive(Debug, Clone)]
struct VirtioCap {
    cfg_type: u8,
    bar: u8,
    offset: u32,
    #[allow(dead_code)]
    length: u32,
    notify_off_multiplier: u32,
}

fn enumerate_virtio_caps(bus: u8, dev: u8, func: u8) -> alloc::vec::Vec<VirtioCap> {
    let mut caps = alloc::vec::Vec::new();
    let cap_ptr = pci_cfg_read_u8(bus, dev, func, 0x34) & 0xFC;
    if cap_ptr == 0 {
        return caps;
    }
    let mut ptr = cap_ptr as u16;
    while ptr != 0 {
        let cap_id = pci_cfg_read_u8(bus, dev, func, ptr);
        if cap_id == CAP_VENDOR_SPECIFIC {
            let cfg_type = pci_cfg_read_u8(bus, dev, func, ptr + 3);
            if (1..=5).contains(&cfg_type) {
                let bar = pci_cfg_read_u8(bus, dev, func, ptr + 4);
                let off = pci_cfg_read_u32(bus, dev, func, ptr + 8);
                let len = pci_cfg_read_u32(bus, dev, func, ptr + 12);
                let mult = if cfg_type == VIRTIO_PCI_CAP_NOTIFY_CFG {
                    pci_cfg_read_u32(bus, dev, func, ptr + 16)
                } else {
                    0
                };
                caps.push(VirtioCap {
                    cfg_type,
                    bar,
                    offset: off,
                    length: len,
                    notify_off_multiplier: mult,
                });
            }
        }
        ptr = pci_cfg_read_u8(bus, dev, func, ptr + 1) as u16 & 0xFC;
    }
    caps
}

fn get_bar_phys(info: &DeviceInfo, bar: u8) -> Option<u64> {
    if bar >= 6 {
        return None;
    }
    match info.bars[bar as usize] {
        Some(Bar::Memory32 { base, .. }) => Some(base as u64),
        Some(Bar::Memory64 { base, .. }) => Some(base),
        _ => None,
    }
}

#[repr(C)]
struct Desc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
struct Avail {
    flags: u16,
    idx: u16,
    ring: [u16; QUEUE_SIZE as usize],
}

#[repr(C)]
struct UsedElem {
    id: u32,
    len: u32,
}

#[repr(C)]
struct Used {
    flags: u16,
    idx: u16,
    ring: [UsedElem; QUEUE_SIZE as usize],
}

struct VirtQueue {
    // SAFETY: VirtQueue is only accessed behind a Mutex. The raw pointers
    // point to heap-allocated memory that remains valid for the lifetime
    // of the device.
    _not_send: core::marker::PhantomData<*mut ()>,
    desc_ptr: *mut Desc,
    avail_ptr: *mut u8,
    used_ptr: *mut u8,
    free_head: u16,
    free_count: u16,
    last_used_idx: u16,
    _queue_idx: u16,
}

unsafe impl Send for VirtQueue {}
unsafe impl Sync for VirtQueue {}

impl VirtQueue {
    fn allocate(
        queue_idx: u16,
        common_cfg_base: u64,
        phys_mem_offset: u64,
    ) -> Option<(Self, u16)> {
        mmio_write_u16(common_cfg_base, COMMON_QUEUE_SEL, queue_idx);
        let size = mmio_read_u16(common_cfg_base, COMMON_QUEUE_SIZE);
        if size < QUEUE_SIZE {
            crate::serial::println!(
                "[VIRTIO] Queue {} size {} < requested {}",
                queue_idx,
                size,
                QUEUE_SIZE
            );
            return None;
        }

        let notify_off = mmio_read_u16(common_cfg_base, COMMON_QUEUE_NOTIFY_OFF);

        let desc_len = core::mem::size_of::<Desc>() * QUEUE_SIZE as usize;
        let avail_len = core::mem::size_of::<Avail>();
        let used_len = core::mem::size_of::<Used>();

        let desc_vec = alloc::vec![0u8; desc_len];
        let avail_vec = alloc::vec![0u8; avail_len];
        let used_vec = alloc::vec![0u8; used_len];

        let desc_virt = desc_vec.as_ptr() as u64;
        let avail_virt = avail_vec.as_ptr() as u64;
        let used_virt = used_vec.as_ptr() as u64;

        mmio_write_u64(common_cfg_base, COMMON_QUEUE_DESC, desc_virt - phys_mem_offset);
        mmio_write_u64(common_cfg_base, COMMON_QUEUE_DRIVER, avail_virt - phys_mem_offset);
        mmio_write_u64(common_cfg_base, COMMON_QUEUE_DEVICE, used_virt - phys_mem_offset);
        mmio_write_u16(common_cfg_base, COMMON_QUEUE_ENABLE, 1);

        let desc_leaked = alloc::boxed::Box::leak(desc_vec.into_boxed_slice());
        let desc_arr = unsafe { &mut *(desc_leaked.as_mut_ptr() as *mut [Desc; QUEUE_SIZE as usize]) };
        for i in 0..QUEUE_SIZE - 1 {
            desc_arr[i as usize].next = i + 1;
        }
        desc_arr[QUEUE_SIZE as usize - 1].next = 0xFFFF;

        let vq = VirtQueue {
            _not_send: core::marker::PhantomData,
            desc_ptr: desc_leaked.as_mut_ptr() as *mut Desc,
            avail_ptr: avail_vec.leak().as_mut_ptr(),
            used_ptr: used_vec.leak().as_mut_ptr(),
            free_head: 0,
            free_count: QUEUE_SIZE,
            last_used_idx: 0,
            _queue_idx: queue_idx,
        };

        Some((vq, notify_off))
    }

    #[allow(dead_code)]
    fn desc_mut(&mut self) -> &mut [Desc] {
        unsafe { core::slice::from_raw_parts_mut(self.desc_ptr, QUEUE_SIZE as usize) }
    }

    fn avail_mut(&mut self) -> &mut Avail {
        unsafe { &mut *(self.avail_ptr as *mut Avail) }
    }

    fn used_ref(&self) -> &Used {
        unsafe { &*(self.used_ptr as *const Used) }
    }

    #[allow(dead_code)]
    fn used_mut(&mut self) -> &mut Used {
        unsafe { &mut *(self.used_ptr as *mut Used) }
    }

    fn alloc_desc(&mut self, count: u16) -> Option<u16> {
        if self.free_count < count {
            return None;
        }
        let desc = unsafe { core::slice::from_raw_parts_mut(self.desc_ptr, QUEUE_SIZE as usize) };
        let head = self.free_head;
        let mut curr = head as usize;
        for _ in 0..count - 1 {
            curr = desc[curr].next as usize;
        }
        self.free_head = desc[curr].next;
        desc[curr].next = 0xFFFF;
        self.free_count -= count;
        Some(head)
    }

    fn free_desc(&mut self, head: u16) {
        let desc = unsafe { core::slice::from_raw_parts_mut(self.desc_ptr, QUEUE_SIZE as usize) };
        let mut curr = head as usize;
        let mut count = 0;
        loop {
            count += 1;
            let next = desc[curr].next;
            desc[curr].next = self.free_head;
            self.free_head = curr as u16;
            if next == 0xFFFF {
                break;
            }
            curr = next as usize;
        }
        self.free_count += count;
    }

    fn make_available(&mut self, head: u16) {
        let avail = self.avail_mut();
        let idx = avail.idx;
        avail.ring[(idx as usize) % QUEUE_SIZE as usize] = head;
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        avail.idx = idx.wrapping_add(1);
    }

    fn has_used(&self) -> bool {
        self.used_ref().idx != self.last_used_idx
    }

    fn pop_used(&mut self) -> Option<(u16, u32)> {
        if !self.has_used() {
            return None;
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
        let used = self.used_ref();
        let elem = &used.ring[self.last_used_idx as usize % QUEUE_SIZE as usize];
        let id = elem.id as u16;
        let len = elem.len;
        self.last_used_idx = self.last_used_idx.wrapping_add(1);
        Some((id, len))
    }
}

struct VirtioNetDevice {
    #[allow(dead_code)]
    common_cfg_base: u64,
    notify_base: u64,
    notify_off_multiplier: u32,
    mac_address: [u8; 6],
    tx_queue: Mutex<VirtQueue>,
    rx_queue: Mutex<VirtQueue>,
    tx_notify_off: u16,
    rx_notify_off: u16,
}

// The device is stored behind a Box inside the Mutex so the Option wraps
// a thin pointer rather than the large struct, avoiding a layout issue
// in the global static initialiser.
static VIRTIO_NET_DEVICE: Mutex<Option<alloc::boxed::Box<VirtioNetDevice>>> = Mutex::new(None);
static VIRTIO_NET_INITIALIZED: AtomicBool = AtomicBool::new(false);

fn read_mac(device_cfg_base: u64) -> [u8; 6] {
    let mut mac = [0u8; 6];
    for i in 0..6u16 {
        mac[i as usize] = mmio_read_u8(device_cfg_base, DEVICE_CFG_MAC_OFFSET + i);
    }
    mac
}

fn init_device(info: &DeviceInfo, caps: &[VirtioCap]) -> Option<VirtioNetDevice> {
    let phys_mem_offset = get_phys_mem_offset();
    let pmo = phys_mem_offset.as_u64();

    let mut common_cfg = None;
    let mut notify_cfg = None;
    let mut isr_cfg = None;
    let mut device_cfg = None;

    for cap in caps {
        let bar_phys = get_bar_phys(info, cap.bar)?;
        let base = pmo + bar_phys + cap.offset as u64;
        match cap.cfg_type {
            VIRTIO_PCI_CAP_COMMON_CFG => common_cfg = Some(base),
            VIRTIO_PCI_CAP_NOTIFY_CFG => {
                notify_cfg = Some((base, cap.notify_off_multiplier));
            }
            VIRTIO_PCI_CAP_ISR_CFG => isr_cfg = Some(base),
            VIRTIO_PCI_CAP_DEVICE_CFG => device_cfg = Some(base),
            _ => {}
        }
    }

    let common_base = common_cfg?;
    let (notify_base, notify_off_multiplier) = notify_cfg?;
    let _isr_base = isr_cfg?;
    let device_cfg_base = device_cfg?;

    mmio_write_u8(common_base, COMMON_DEVICE_STATUS, 0);
    let mut retries = 100;
    while mmio_read_u8(common_base, COMMON_DEVICE_STATUS) != 0 {
        retries -= 1;
        if retries == 0 {
            crate::serial::println!("[VIRTIO] Reset failed");
            return None;
        }
        core::hint::spin_loop();
    }

    mmio_write_u8(common_base, COMMON_DEVICE_STATUS, STATUS_ACK);
    mmio_write_u8(common_base, COMMON_DEVICE_STATUS, STATUS_ACK | STATUS_DRIVER);

    mmio_write_u32(common_base, COMMON_DEVICE_FEATURE_SEL, 0);
    let dev_features_low = mmio_read_u32(common_base, COMMON_DEVICE_FEATURES);
    mmio_write_u32(common_base, COMMON_DEVICE_FEATURE_SEL, 1);
    let dev_features_high = mmio_read_u32(common_base, COMMON_DEVICE_FEATURES);
    let dev_features = (dev_features_high as u64) << 32 | dev_features_low as u64;

    if dev_features & VIRTIO_F_VERSION_1 == 0 {
        crate::serial::println!("[VIRTIO] No VIRTIO_F_VERSION_1 support");
        mmio_write_u8(common_base, COMMON_DEVICE_STATUS, STATUS_FAILED);
        return None;
    }

    let mut driver_features: u64 = VIRTIO_F_VERSION_1;
    if dev_features & VIRTIO_NET_F_MAC != 0 {
        driver_features |= VIRTIO_NET_F_MAC;
    }

    mmio_write_u32(common_base, COMMON_DRIVER_FEATURE_SEL, 0);
    mmio_write_u32(common_base, COMMON_DRIVER_FEATURES, driver_features as u32);
    mmio_write_u32(common_base, COMMON_DRIVER_FEATURE_SEL, 1);
    mmio_write_u32(common_base, COMMON_DRIVER_FEATURES, (driver_features >> 32) as u32);

    mmio_write_u8(
        common_base,
        COMMON_DEVICE_STATUS,
        STATUS_ACK | STATUS_DRIVER | STATUS_FEATURES_OK,
    );

    let status = mmio_read_u8(common_base, COMMON_DEVICE_STATUS);
    if status & STATUS_FEATURES_OK == 0 {
        crate::serial::println!("[VIRTIO] Feature negotiation failed (0x{:02x})", status);
        mmio_write_u8(common_base, COMMON_DEVICE_STATUS, STATUS_FAILED);
        return None;
    }

    let mac_address = read_mac(device_cfg_base);

    let (tx_queue, tx_notify_off) =
        VirtQueue::allocate(0, common_base, pmo)?;
    let (rx_queue, rx_notify_off) =
        VirtQueue::allocate(1, common_base, pmo)?;

    mmio_write_u8(
        common_base,
        COMMON_DEVICE_STATUS,
        STATUS_ACK | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
    );

    crate::serial::println!(
        "[VIRTIO] MAC={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac_address[0], mac_address[1], mac_address[2],
        mac_address[3], mac_address[4], mac_address[5],
    );

    Some(VirtioNetDevice {
        common_cfg_base: common_base,
        notify_base,
        notify_off_multiplier,
        mac_address,
        tx_queue: Mutex::new(tx_queue),
        rx_queue: Mutex::new(rx_queue),
        tx_notify_off,
        rx_notify_off,
    })
}

pub fn get_mac_address() -> Option<[u8; 6]> {
    let guard = VIRTIO_NET_DEVICE.lock();
    guard.as_ref().map(|b| b.mac_address)
}

pub fn is_initialized() -> bool {
    VIRTIO_NET_INITIALIZED.load(Ordering::Acquire)
}

pub fn transmit_packet(data: &[u8]) -> bool {
    let mut guard = VIRTIO_NET_DEVICE.lock();
    let dev = match guard.as_mut() {
        Some(b) => b.as_mut(),
        None => return false,
    };

    let pmo = get_phys_mem_offset().as_u64();
    let mut tx = dev.tx_queue.lock();

    let head = match tx.alloc_desc(1) {
        Some(h) => h,
        None => {
            reclaim_tx(&mut tx);
            match tx.alloc_desc(1) {
                Some(h) => h,
                None => return false,
            }
        }
    };

    let buf = data.to_vec();
    let buf_phys = buf.as_ptr() as u64 - pmo;
    core::mem::forget(buf);

    let desc = unsafe { core::slice::from_raw_parts_mut(tx.desc_ptr, QUEUE_SIZE as usize) };
    desc[head as usize].addr = buf_phys;
    desc[head as usize].len = data.len() as u32;
    desc[head as usize].flags = 0;
    desc[head as usize].next = 0xFFFF;

    tx.make_available(head);

    let notify_addr = dev.notify_base
        + dev.notify_off_multiplier as u64 * dev.tx_notify_off as u64;
    mmio_write_u16(notify_addr, 0, 0);

    true
}

fn reclaim_tx(tx: &mut VirtQueue) {
    while let Some((id, _len)) = tx.pop_used() {
        tx.free_desc(id);
    }
}

pub fn poll_rx<F: FnMut(&[u8])>(mut callback: F) {
    let mut guard = VIRTIO_NET_DEVICE.lock();
    let dev = match guard.as_mut() {
        Some(b) => b.as_mut(),
        None => return,
    };

    let pmo = get_phys_mem_offset().as_u64();
    let mut rx = dev.rx_queue.lock();

    while let Some((id, len)) = rx.pop_used() {
        let desc = unsafe { core::slice::from_raw_parts_mut(rx.desc_ptr, QUEUE_SIZE as usize) };
        let virt_ptr = (desc[id as usize].addr + pmo) as *const u8;
        let slice = unsafe { core::slice::from_raw_parts(virt_ptr, len as usize) };
        callback(slice);
        rx.free_desc(id);
        let buf = alloc::vec![0u8; 2048];
        let buf_phys = buf.as_ptr() as u64 - pmo;
        core::mem::forget(buf);
        let new_head = rx.alloc_desc(1).unwrap_or(0xFFFF);
        if new_head != 0xFFFF {
            let rdesc = unsafe { core::slice::from_raw_parts_mut(rx.desc_ptr, QUEUE_SIZE as usize) };
            rdesc[new_head as usize].addr = buf_phys;
            rdesc[new_head as usize].len = 2048;
            rdesc[new_head as usize].flags = 2;
            rdesc[new_head as usize].next = 0xFFFF;
            rx.make_available(new_head);
            let notify_addr = dev.notify_base
                + dev.notify_off_multiplier as u64 * dev.rx_notify_off as u64;
            mmio_write_u16(notify_addr, 0, 1);
        }
    }
}

pub fn reinit() -> bool {
    let device_registry = crate::drivers::DEVICE_REGISTRY.lock();
    let mut dev_guard = VIRTIO_NET_DEVICE.lock();

    for (_, info) in device_registry.iter_device_infos() {
        let is_virtio_net = info.vendor_id == VIRTIO_VENDOR
            && (info.device_id == VIRTIO_NET_TRANSITIONAL
                || info.device_id == VIRTIO_NET_MODERN);
        if !is_virtio_net {
            continue;
        }
        let caps = enumerate_virtio_caps(info.bus, info.device, info.function);
        if let Some(dev) = init_device(info, &caps) {
            *dev_guard = Some(alloc::boxed::Box::new(dev));
            VIRTIO_NET_INITIALIZED.store(true, Ordering::Release);
            crate::serial::println!("[VIRTIO] Re-initialisation succeeded");
            return true;
        }
    }
    crate::serial::println!("[VIRTIO] Re-initialisation failed");
    false
}

#[derive(Debug)]
pub struct VirtioNetError;

pub struct VirtioNetDriver;

impl DeviceDriver for VirtioNetDriver {
    type Config = ();
    type Error = VirtioNetError;

    fn probe(info: &DeviceInfo) -> Result<Self, Self::Error> {
        if info.vendor_id != VIRTIO_VENDOR {
            return Err(VirtioNetError);
        }
        let is_net = info.device_id == VIRTIO_NET_TRANSITIONAL
            || info.device_id == VIRTIO_NET_MODERN;
        if !is_net {
            return Err(VirtioNetError);
        }

        let caps = enumerate_virtio_caps(info.bus, info.device, info.function);
        if caps.is_empty() {
            return Err(VirtioNetError);
        }

        let device = init_device(info, &caps).ok_or(VirtioNetError)?;

        let mut dev_guard = VIRTIO_NET_DEVICE.lock();
        *dev_guard = Some(alloc::boxed::Box::new(device));
        VIRTIO_NET_INITIALIZED.store(true, Ordering::Release);
        Ok(VirtioNetDriver)
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
        "virtio-net"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_queue_size_constant() {
        assert_eq!(QUEUE_SIZE, 256);
    }

    #[test]
    fn test_feature_mask_values() {
        assert_eq!(VIRTIO_NET_F_MAC, 1 << 5);
        assert_eq!(VIRTIO_F_VERSION_1, 1 << 32);
    }

    #[test]
    fn test_virtio_cap_type_values() {
        assert_eq!(VIRTIO_PCI_CAP_COMMON_CFG, 1);
        assert_eq!(VIRTIO_PCI_CAP_NOTIFY_CFG, 2);
        assert_eq!(VIRTIO_PCI_CAP_ISR_CFG, 3);
        assert_eq!(VIRTIO_PCI_CAP_DEVICE_CFG, 4);
    }

    #[test]
    fn test_device_status_values() {
        assert_eq!(STATUS_ACK, 0x01);
        assert_eq!(STATUS_DRIVER, 0x02);
        assert_eq!(STATUS_DRIVER_OK, 0x04);
        assert_eq!(STATUS_FEATURES_OK, 0x08);
        assert_eq!(STATUS_FAILED, 0x80);
    }

    #[test]
    fn test_desc_size() {
        assert_eq!(core::mem::size_of::<Desc>(), 16);
    }

    #[test]
    fn test_virtqueue_free_list_logic() {
        let desc_len = core::mem::size_of::<Desc>() * QUEUE_SIZE as usize;

        let mut desc_mem = alloc::vec![0u8; desc_len];
        let desc = unsafe { &mut *(desc_mem.as_mut_ptr() as *mut [Desc; QUEUE_SIZE as usize]) };
        for i in 0..QUEUE_SIZE - 1 {
            desc[i as usize].next = i + 1;
        }
        desc[QUEUE_SIZE as usize - 1].next = 0xFFFF;

        let mut free_head = 0u16;
        let mut free_count = QUEUE_SIZE;

        for expected in 0..5u16 {
            assert!(free_count > 0);
            assert_eq!(free_head, expected);
            let head = free_head;
            free_head = desc[free_head as usize].next;
            desc[head as usize].next = 0xFFFF;
            free_count -= 1;
            assert_eq!(head, expected);
        }
        assert_eq!(free_count, QUEUE_SIZE - 5);

        for i in (0..5u16).rev() {
            desc[i as usize].next = free_head;
            free_head = i;
            free_count += 1;
        }
        assert_eq!(free_count, QUEUE_SIZE);

        let heads: alloc::vec::Vec<_> = (0..5)
            .map(|_| {
                let h = free_head;
                free_head = desc[free_head as usize].next;
                desc[h as usize].next = 0xFFFF;
                free_count -= 1;
                h
            })
            .collect();
        assert_eq!(heads, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_capability_detection_no_bus() {
        let vid = 0xFFFFu16;
        let did = 0xFFFFu16;
        let is_virtio_net = vid == VIRTIO_VENDOR
            && (did == VIRTIO_NET_TRANSITIONAL || did == VIRTIO_NET_MODERN);
        assert!(!is_virtio_net);
    }

    #[test]
    fn test_mac_format() {
        let mac: [u8; 6] = [0x02, 0x00, 0xAD, 0xDE, 0x00, 0x01];
        assert_eq!(mac.len(), 6);
        assert_eq!(mac[0], 0x02);
    }

    #[test]
    fn test_probe_rejection_non_virtio() {
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
        assert!(VirtioNetDriver::probe(&info).is_err());
    }
}
