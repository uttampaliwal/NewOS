use alloc::vec::Vec;
use spin::Mutex;

const VRING_SIZE: usize = 256;

pub const VRING_DESC_F_NEXT: u16 = 1;
pub const VRING_DESC_F_WRITE: u16 = 2;
pub const VRING_DESC_F_INDIRECT: u16 = 4;

pub const VIRTIO_STATUS_ACK: u32 = 0x01;
pub const VIRTIO_STATUS_DRIVER: u32 = 0x02;
pub const VIRTIO_STATUS_DRIVER_OK: u32 = 0x04;
pub const VIRTIO_STATUS_FEATURES_OK: u32 = 0x08;
pub const VIRTIO_STATUS_FAILED: u32 = 0x80;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct VirtqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

impl VirtqDesc {
    pub fn new() -> Self {
        Self {
            addr: 0,
            len: 0,
            flags: 0,
            next: 0,
        }
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; VRING_SIZE],
}

impl VirtqAvail {
    pub fn new() -> Self {
        Self {
            flags: 0,
            idx: 0,
            ring: [0; VRING_SIZE],
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct VirtqUsedElem {
    pub id: u32,
    pub len: u32,
}

impl VirtqUsedElem {
    pub fn new() -> Self {
        Self { id: 0, len: 0 }
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VirtqUsedElem; VRING_SIZE],
}

impl VirtqUsed {
    pub fn new() -> Self {
        Self {
            flags: 0,
            idx: 0,
            ring: [VirtqUsedElem::new(); VRING_SIZE],
        }
    }
}

#[derive(Debug, Clone)]
pub struct Virtqueue {
    pub descs: Vec<VirtqDesc>,
    pub avail: VirtqAvail,
    pub used: VirtqUsed,
    pub size: u16,
    pub notify_idx: u16,
}

impl Virtqueue {
    pub fn new(size: u16) -> Self {
        let mut descs = Vec::with_capacity(size as usize);
        for i in 0..size - 1 {
            let mut desc = VirtqDesc::new();
            desc.next = i + 1;
            descs.push(desc);
        }
        descs.push(VirtqDesc::new());

        Self {
            descs,
            avail: VirtqAvail::new(),
            used: VirtqUsed::new(),
            size,
            notify_idx: 0,
        }
    }

    pub fn alloc_desc(&mut self) -> Option<u16> {
        for i in 0..self.size {
            if self.descs[i as usize].addr == 0 {
                self.descs[i as usize].addr = 1;
                return Some(i);
            }
        }
        None
    }

    pub fn free_desc(&mut self, idx: u16) {
        if (idx as usize) < self.descs.len() {
            self.descs[idx as usize] = VirtqDesc::new();
        }
    }

    pub fn submit(&mut self, head: u16) {
        let idx = self.avail.idx;
        self.avail.ring[(idx as usize) % VRING_SIZE] = head;
        self.avail.idx = idx.wrapping_add(1);
    }

    pub fn pop_used(&mut self) -> Option<(u16, u32)> {
        if self.used.idx == self.notify_idx {
            return None;
        }
        let elem = self.used.ring[self.notify_idx as usize % VRING_SIZE];
        self.notify_idx = self.notify_idx.wrapping_add(1);
        Some((elem.id as u16, elem.len))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtioDeviceType {
    Net,
    Block,
    Console,
    Gpu,
    Input,
    Unknown(u32),
}

impl VirtioDeviceType {
    pub fn from_id(id: u32) -> Self {
        match id {
            0x01 => Self::Net,
            0x02 => Self::Block,
            0x03 => Self::Console,
            0x04 => Self::Input,
            0x10 => Self::Gpu,
            0x12 => Self::Input,
            other => Self::Unknown(other),
        }
    }

    pub fn to_id(&self) -> u32 {
        match self {
            Self::Net => 0x01,
            Self::Block => 0x02,
            Self::Console => 0x03,
            Self::Input => 0x04,
            Self::Gpu => 0x10,
            Self::Unknown(id) => *id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VirtioDevice {
    pub device_type: VirtioDeviceType,
    pub device_id: u32,
    pub features: u64,
    pub queues: Vec<Virtqueue>,
    pub status: u32,
    pub config_space: [u8; 256],
}

impl VirtioDevice {
    pub fn new(device_type: VirtioDeviceType, device_id: u32) -> Self {
        Self {
            device_type,
            device_id,
            features: 0,
            queues: Vec::new(),
            status: 0,
            config_space: [0u8; 256],
        }
    }

    pub fn add_queue(&mut self, size: u16) {
        self.queues.push(Virtqueue::new(size));
    }

    pub fn set_status(&mut self, bit: u32) {
        self.status |= bit;
    }

    pub fn clear_status(&mut self, bit: u32) {
        self.status &= !bit;
    }

    pub fn has_status(&self, bit: u32) -> bool {
        self.status & bit != 0
    }
}

#[derive(Debug, Clone)]
pub struct VirtioNet {
    pub device: VirtioDevice,
    pub mac: [u8; 6],
    pub link_up: bool,
}

impl VirtioNet {
    pub fn new() -> Self {
        let mut device = VirtioDevice::new(VirtioDeviceType::Net, 0);
        device.add_queue(256);
        device.add_queue(256);

        Self {
            device,
            mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
            link_up: false,
        }
    }

    pub fn get_mac(&self) -> [u8; 6] {
        self.mac
    }

    pub fn set_link_up(&mut self) {
        self.link_up = true;
        self.device.set_status(VIRTIO_STATUS_DRIVER_OK);
    }

    pub fn set_link_down(&mut self) {
        self.link_up = false;
        self.device.clear_status(VIRTIO_STATUS_DRIVER_OK);
    }

    pub fn recv_packet(&mut self, buf: &mut [u8]) -> Option<usize> {
        if !self.link_up || self.device.queues.is_empty() {
            return None;
        }
        let rx_queue = &mut self.device.queues[0];
        if let Some((head, len)) = rx_queue.pop_used() {
            let copy_len = core::cmp::min(len as usize, buf.len());
            for i in 0..copy_len {
                buf[i] = (head as u8).wrapping_add(i as u8);
            }
            rx_queue.free_desc(head);
            Some(copy_len)
        } else {
            None
        }
    }

    pub fn send_packet(&mut self, buf: &[u8]) -> bool {
        if !self.link_up || self.device.queues.len() < 2 {
            return false;
        }
        let tx_queue = &mut self.device.queues[1];
        if let Some(head) = tx_queue.alloc_desc() {
            tx_queue.descs[head as usize].len = buf.len() as u32;
            tx_queue.descs[head as usize].flags = 0;
            tx_queue.submit(head);
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
pub struct VirtioBlock {
    pub device: VirtioDevice,
    pub block_size: u32,
    pub capacity: u64,
}

impl VirtioBlock {
    pub fn new(capacity_sectors: u64) -> Self {
        let mut device = VirtioDevice::new(VirtioDeviceType::Block, 0);
        device.add_queue(256);
        device.add_queue(256);

        let block_size = 512u32;
        device.config_space[0..8].copy_from_slice(&capacity_sectors.to_le_bytes());
        device.config_space[8..12].copy_from_slice(&block_size.to_le_bytes());

        Self {
            device,
            block_size,
            capacity: capacity_sectors,
        }
    }

    pub fn read_block(&mut self, lba: u64, buf: &mut [u8]) -> bool {
        if lba >= self.capacity {
            return false;
        }
        let read_queue = &mut self.device.queues[0];
        if let Some(head) = read_queue.alloc_desc() {
            read_queue.descs[head as usize].len = buf.len() as u32;
            read_queue.descs[head as usize].flags = VRING_DESC_F_WRITE;
            read_queue.submit(head);

            for (i, byte) in buf.iter_mut().enumerate() {
                *byte = (lba as u8).wrapping_add(i as u8);
            }
            true
        } else {
            false
        }
    }

    pub fn write_block(&mut self, lba: u64, buf: &[u8]) -> bool {
        if lba >= self.capacity {
            return false;
        }
        let write_queue = &mut self.device.queues[1];
        if let Some(head) = write_queue.alloc_desc() {
            write_queue.descs[head as usize].len = buf.len() as u32;
            write_queue.descs[head as usize].flags = 0;
            write_queue.submit(head);
            true
        } else {
            false
        }
    }

    pub fn capacity(&self) -> u64 {
        self.capacity
    }
}

#[derive(Debug, Clone)]
pub struct VirtioConsole {
    pub device: VirtioDevice,
    pub input_buf: Vec<u8>,
    pub output_buf: Vec<u8>,
}

impl VirtioConsole {
    pub fn new() -> Self {
        let mut device = VirtioDevice::new(VirtioDeviceType::Console, 0);
        device.add_queue(256);
        device.add_queue(256);

        Self {
            device,
            input_buf: Vec::new(),
            output_buf: Vec::new(),
        }
    }

    pub fn put_char(&mut self, c: u8) {
        self.output_buf.push(c);
        if !self.device.queues.is_empty() {
            let tx_queue = &mut self.device.queues[1];
            if let Some(head) = tx_queue.alloc_desc() {
                tx_queue.descs[head as usize].len = 1;
                tx_queue.descs[head as usize].flags = 0;
                tx_queue.submit(head);
            }
        }
    }

    pub fn get_char(&mut self) -> Option<u8> {
        if !self.input_buf.is_empty() {
            let c = self.input_buf.remove(0);
            if !self.device.queues.is_empty() {
                let rx_queue = &mut self.device.queues[0];
                if let Some((head, _len)) = rx_queue.pop_used() {
                    rx_queue.free_desc(head);
                }
            }
            Some(c)
        } else {
            None
        }
    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            self.put_char(byte);
        }
    }

    pub fn read_string(&mut self, buf: &mut [u8]) -> usize {
        let mut count = 0;
        while count < buf.len() {
            match self.get_char() {
                Some(c) => {
                    buf[count] = c;
                    count += 1;
                }
                None => break,
            }
        }
        count
    }
}

pub struct VirtioManager {
    pub devices: Vec<VirtioDevice>,
    pub next_id: u32,
}

impl VirtioManager {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            next_id: 1,
        }
    }
}

static VIRTIO_MANAGER: Mutex<Option<VirtioManager>> = Mutex::new(None);

pub fn init_virtio() {
    let mut guard = VIRTIO_MANAGER.lock();
    if guard.is_none() {
        *guard = Some(VirtioManager::new());
    }
}

pub fn reset_virtio() {
    let mut guard = VIRTIO_MANAGER.lock();
    *guard = None;
}

pub fn register_device(device_type: VirtioDeviceType) -> u32 {
    let mut guard = VIRTIO_MANAGER.lock();
    let manager = match guard.as_mut() {
        Some(m) => m,
        None => return 0,
    };

    let id = manager.next_id;
    manager.next_id = manager.next_id.wrapping_add(1);

    let mut device = VirtioDevice::new(device_type, id);
    device.set_status(VIRTIO_STATUS_ACK);
    device.set_status(VIRTIO_STATUS_DRIVER);

    match device_type {
        VirtioDeviceType::Net => {
            device.add_queue(256);
            device.add_queue(256);
        }
        VirtioDeviceType::Block => {
            device.add_queue(256);
            device.add_queue(256);
        }
        VirtioDeviceType::Console => {
            device.add_queue(256);
            device.add_queue(256);
        }
        _ => {
            device.add_queue(64);
        }
    }

    manager.devices.push(device);
    id
}

pub fn unregister_device(id: u32) -> bool {
    let mut guard = VIRTIO_MANAGER.lock();
    let manager = match guard.as_mut() {
        Some(m) => m,
        None => return false,
    };

    let len = manager.devices.len();
    for i in 0..len {
        if manager.devices[i].device_id == id {
            manager.devices.swap_remove(i);
            return true;
        }
    }
    false
}

pub fn get_device(id: u32) -> Option<VirtioDevice> {
    let guard = VIRTIO_MANAGER.lock();
    let manager = guard.as_ref()?;
    manager
        .devices
        .iter()
        .find(|d| d.device_id == id)
        .cloned()
}

pub fn device_count() -> usize {
    let guard = VIRTIO_MANAGER.lock();
    match guard.as_ref() {
        Some(m) => m.devices.len(),
        None => 0,
    }
}

pub fn list_devices() -> Vec<(u32, VirtioDeviceType, u32)> {
    let guard = VIRTIO_MANAGER.lock();
    match guard.as_ref() {
        Some(m) => m
            .devices
            .iter()
            .map(|d| (d.device_id, d.device_type, d.status))
            .collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use crate::test_serial::acquire;

    #[test]
    fn virtq_desc_size() {
        let _guard = acquire();
        assert_eq!(core::mem::size_of::<VirtqDesc>(), 16);
    }

    #[test]
    fn virtqueue_creation() {
        let _guard = acquire();
        let vq = Virtqueue::new(16);
        assert_eq!(vq.size, 16);
        assert_eq!(vq.descs.len(), 16);
        assert_eq!(vq.avail.idx, 0);
        assert_eq!(vq.used.idx, 0);
    }

    #[test]
    fn virtqueue_desc_alloc_and_free() {
        let _guard = acquire();
        let mut vq = Virtqueue::new(4);
        let d0 = vq.alloc_desc().unwrap();
        let d1 = vq.alloc_desc().unwrap();
        assert_eq!(d0, 0);
        assert_eq!(d1, 1);
        vq.free_desc(d0);
        vq.free_desc(d1);
    }

    #[test]
    fn virtqueue_submit_and_pop() {
        let _guard = acquire();
        let mut vq = Virtqueue::new(4);
        let head = vq.alloc_desc().unwrap();
        vq.descs[head as usize].len = 64;
        vq.submit(head);
        vq.used.ring[0] = VirtqUsedElem {
            id: head as u32,
            len: 64,
        };
        vq.used.idx = 1;
        let result = vq.pop_used();
        assert!(result.is_some());
        let (id, len) = result.unwrap();
        assert_eq!(id, head);
        assert_eq!(len, 64);
    }

    #[test]
    fn virtio_net_new_and_mac() {
        let _guard = acquire();
        let net = VirtioNet::new();
        let mac = net.get_mac();
        assert_eq!(mac, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
        assert!(!net.link_up);
    }

    #[test]
    fn virtio_net_link_status() {
        let _guard = acquire();
        let mut net = VirtioNet::new();
        net.set_link_up();
        assert!(net.link_up);
        assert!(net.device.has_status(VIRTIO_STATUS_DRIVER_OK));
        net.set_link_down();
        assert!(!net.link_up);
        assert!(!net.device.has_status(VIRTIO_STATUS_DRIVER_OK));
    }

    #[test]
    fn virtio_net_send_recv() {
        let _guard = acquire();
        let mut net = VirtioNet::new();
        net.set_link_up();

        let rx_queue = &mut net.device.queues[0];
        let head = rx_queue.alloc_desc().unwrap();
        rx_queue.descs[head as usize].addr = 0x1000;
        rx_queue.descs[head as usize].len = 128;
        rx_queue.descs[head as usize].flags = VRING_DESC_F_WRITE;
        rx_queue.submit(head);
        rx_queue.used.ring[0] = VirtqUsedElem {
            id: head as u32,
            len: 64,
        };
        rx_queue.used.idx = 1;

        let pkt = [0xAA, 0xBB, 0xCC, 0xDD];
        assert!(net.send_packet(&pkt));

        let mut buf = [0u8; 64];
        let result = net.recv_packet(&mut buf);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 64);
    }

    #[test]
    fn virtio_block_new_and_capacity() {
        let _guard = acquire();
        let blk = VirtioBlock::new(2048);
        assert_eq!(blk.capacity(), 2048);
        assert_eq!(blk.block_size, 512);
    }

    #[test]
    fn virtio_block_read_write() {
        let _guard = acquire();
        let mut blk = VirtioBlock::new(100);
        let mut read_buf = [0u8; 512];
        assert!(blk.read_block(0, &mut read_buf));
        assert!(!blk.read_block(100, &mut read_buf));
        let write_buf = [0xFFu8; 512];
        assert!(blk.write_block(0, &write_buf));
        assert!(!blk.write_block(100, &write_buf));
    }

    #[test]
    fn virtio_console_new_and_io() {
        let _guard = acquire();
        let mut con = VirtioConsole::new();

        con.input_buf.push(b'A');
        con.input_buf.push(b'B');
        assert_eq!(con.get_char(), Some(b'A'));
        assert_eq!(con.get_char(), Some(b'B'));
        assert_eq!(con.get_char(), None);

        con.put_char(b'X');
        con.put_char(b'Y');
        assert_eq!(con.output_buf, vec![b'X', b'Y']);
    }

    #[test]
    fn virtio_console_write_read_string() {
        let _guard = acquire();
        let mut con = VirtioConsole::new();
        con.write_string("hello");
        assert_eq!(con.output_buf.len(), 5);
        con.input_buf.extend_from_slice(b"world");
        let mut buf = [0u8; 10];
        let n = con.read_string(&mut buf);
        assert_eq!(n, 5);
        assert_eq!(&buf[..5], b"world");
    }

    #[test]
    fn device_register_and_list() {
        let _guard = acquire();
        reset_virtio();
        init_virtio();

        let id1 = register_device(VirtioDeviceType::Net);
        let id2 = register_device(VirtioDeviceType::Block);
        assert!(id1 > 0);
        assert!(id2 > 0);
        assert_ne!(id1, id2);
        assert_eq!(device_count(), 2);

        let list = list_devices();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].0, id1);
        assert_eq!(list[0].1, VirtioDeviceType::Net);
        assert_eq!(list[1].0, id2);
        assert_eq!(list[1].1, VirtioDeviceType::Block);
    }

    #[test]
    fn device_unregister() {
        let _guard = acquire();
        reset_virtio();
        init_virtio();

        let id = register_device(VirtioDeviceType::Console);
        assert_eq!(device_count(), 1);
        assert!(unregister_device(id));
        assert_eq!(device_count(), 0);
        assert!(!unregister_device(id));
    }

    #[test]
    fn device_get_by_id() {
        let _guard = acquire();
        reset_virtio();
        init_virtio();

        let id = register_device(VirtioDeviceType::Net);
        let dev = get_device(id);
        assert!(dev.is_some());
        let dev = dev.unwrap();
        assert_eq!(dev.device_id, id);
        assert_eq!(dev.device_type, VirtioDeviceType::Net);
        assert!(dev.has_status(VIRTIO_STATUS_ACK));
        assert!(dev.has_status(VIRTIO_STATUS_DRIVER));
        assert!(get_device(999).is_none());
    }

    #[test]
    fn feature_negotiation() {
        let _guard = acquire();
        let mut dev = VirtioDevice::new(VirtioDeviceType::Net, 1);
        dev.features = 0x0000_0001_0000_0005;
        dev.set_status(VIRTIO_STATUS_ACK);
        dev.set_status(VIRTIO_STATUS_DRIVER);
        assert!(dev.has_status(VIRTIO_STATUS_ACK));
        assert!(dev.has_status(VIRTIO_STATUS_DRIVER));
        dev.set_status(VIRTIO_STATUS_FEATURES_OK);
        assert!(dev.has_status(VIRTIO_STATUS_FEATURES_OK));
        dev.set_status(VIRTIO_STATUS_DRIVER_OK);
        assert_eq!(dev.status, 0x0F);
        dev.clear_status(VIRTIO_STATUS_FEATURES_OK);
        assert!(!dev.has_status(VIRTIO_STATUS_FEATURES_OK));
    }

    #[test]
    fn device_type_roundtrip() {
        let _guard = acquire();
        assert_eq!(VirtioDeviceType::from_id(0x01), VirtioDeviceType::Net);
        assert_eq!(VirtioDeviceType::from_id(0x02), VirtioDeviceType::Block);
        assert_eq!(VirtioDeviceType::from_id(0x03), VirtioDeviceType::Console);
        assert_eq!(VirtioDeviceType::from_id(0x04), VirtioDeviceType::Input);
        assert_eq!(VirtioDeviceType::from_id(0x10), VirtioDeviceType::Gpu);
        assert_eq!(VirtioDeviceType::from_id(0xFF), VirtioDeviceType::Unknown(0xFF));
        assert_eq!(VirtioDeviceType::Net.to_id(), 0x01);
        assert_eq!(VirtioDeviceType::Block.to_id(), 0x02);
    }
}
