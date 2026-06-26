//! VirtIO-GPU hardware driver.
//!
//! Communicates with VirtIO-GPU hardware through virtqueues via the transport layer.

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use super::transport::{
    TransportError, VirtioTransport, Virtqueue,
    VIRTQ_DESC_F_NEXT, VIRTQ_DESC_F_WRITE,
    VIRTIO_STATUS_ACK, VIRTIO_STATUS_DRIVER, VIRTIO_STATUS_DRIVER_OK,
    VIRTIO_STATUS_FEATURES_OK, VIRTIO_STATUS_FAILED,
};

/// VirtIO-GPU controlq request types.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuCmdType {
    GetDisplayInfo = 0x100,
    SetScanout = 0x101,
    ResourceCreate2d = 0x102,
    TransferToHost2d = 0x103,
    ResourceFlush2d = 0x104,
    ResourceAttachBacking = 0x105,
    ResourceDetachBacking = 0x106,
    GetEDID = 0x110,
    RespOkDisplayInfo = 0x1100,
    RespOkNodata = 0x1101,
    RespErrUnspec = 0x1200,
}

/// VirtIO-GPU controlq header (24 bytes).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct GpuCtrlHdr {
    pub ctrl_type: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub padding: u32,
}

impl GpuCtrlHdr {
    pub fn new(ctrl_type: GpuCmdType) -> Self {
        Self {
            ctrl_type: ctrl_type as u32,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        }
    }
}

/// Resource create 2D command payload.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ResourceCreate2d {
    pub header: GpuCtrlHdr,
    pub resource_id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

/// Resource attach backing command payload.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ResourceAttachBacking {
    pub header: GpuCtrlHdr,
    pub resource_id: u32,
    pub nr_entries: u32,
}

/// Memory entry for resource attach backing.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MemEntry {
    pub addr: u64,
    pub length: u32,
    pub padding: u32,
}

/// Set scanout command payload.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SetScanout {
    pub header: GpuCtrlHdr,
    pub scanout_id: u32,
    pub resource_id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Transfer to host 2D command payload.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct TransferToHost2d {
    pub header: GpuCtrlHdr,
    pub resource_id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Resource flush 2D command payload.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ResourceFlush2d {
    pub header: GpuCtrlHdr,
    pub resource_id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Display info response.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct DisplayInfoResponse {
    pub header: GpuCtrlHdr,
    pub pmodes: [DisplayOne; 16],
}

/// Single display mode info.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DisplayOne {
    pub xres: u32,
    pub yres: u32,
    pub xoff: u32,
    pub yoff: u32,
}

/// GPU pixel formats (DRM fourcc).
pub mod formats {
    pub const DRM_FORMAT_XRGB8888: u32 = 0x34324258;
    pub const DRM_FORMAT_ARGB8888: u32 = 0x34324152;
    pub const DRM_FORMAT_RGB565: u32 = 0x36314752;
}

/// VirtIO-GPU errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtioGpuError {
    Transport(TransportError),
    DeviceFailed,
    InvalidResource,
    InvalidScanout,
    UnsupportedFormat,
    CommandFailed(u32),
    Timeout,
}

impl From<TransportError> for VirtioGpuError {
    fn from(e: TransportError) -> Self {
        VirtioGpuError::Transport(e)
    }
}

/// GPU resource tracked by the driver.
#[derive(Debug, Clone)]
pub struct GpuResource {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub backing_addr: Option<u64>,
    pub backing_size: u64,
}

/// VirtIO-GPU hardware driver.
pub struct VirtioGpuHardwareDriver {
    transport: Box<dyn VirtioTransport>,
    controlq: Virtqueue,
    cursorq: Virtqueue,
    resources: Vec<GpuResource>,
    next_resource_id: u32,
    display_width: u32,
    display_height: u32,
    scanout_resource: Option<u32>,
    next_fence_id: u64,
    initialized: bool,
    flip_complete: Option<alloc::sync::Arc<AtomicBool>>,
}

impl VirtioGpuHardwareDriver {
    pub fn new(transport: Box<dyn VirtioTransport>) -> Self {
        Self {
            transport,
            controlq: Virtqueue::new(0),
            cursorq: Virtqueue::new(1),
            resources: Vec::new(),
            next_resource_id: 1,
            display_width: 0,
            display_height: 0,
            scanout_resource: None,
            next_fence_id: 1,
            initialized: false,
            flip_complete: None,
        }
    }

    pub fn initialize(&mut self) -> Result<(), VirtioGpuError> {
        self.transport.reset();
        self.transport.add_status(VIRTIO_STATUS_ACK);
        self.transport.add_status(VIRTIO_STATUS_DRIVER);

        let device_features = self.transport.get_features();
        self.transport.set_features(device_features & 0);

        self.transport.add_status(VIRTIO_STATUS_FEATURES_OK);
        if self.transport.get_status() & VIRTIO_STATUS_FEATURES_OK == 0 {
            self.transport.add_status(VIRTIO_STATUS_FAILED);
            return Err(VirtioGpuError::DeviceFailed);
        }

        self.transport.add_status(VIRTIO_STATUS_DRIVER_OK);
        self.initialized = true;
        Ok(())
    }

    fn send_command<T: Copy>(
        &mut self,
        cmd: &T,
        resp_buf: &mut [u8],
    ) -> Result<(), VirtioGpuError> {
        if !self.initialized {
            return Err(VirtioGpuError::DeviceFailed);
        }

        let cmd_desc = self.controlq.alloc_desc().ok_or(
            VirtioGpuError::Transport(TransportError::NoFreeDescriptors),
        )?;

        let cmd_size = core::mem::size_of::<T>();
        self.controlq.descriptors[cmd_desc].addr = cmd as *const T as u64;
        self.controlq.descriptors[cmd_desc].len = cmd_size as u32;
        self.controlq.descriptors[cmd_desc].flags = 0;

        let resp_desc = self.controlq.alloc_desc().ok_or(
            VirtioGpuError::Transport(TransportError::NoFreeDescriptors),
        )?;

        self.controlq.descriptors[resp_desc].addr = resp_buf.as_ptr() as u64;
        self.controlq.descriptors[resp_desc].len = resp_buf.len() as u32;
        self.controlq.descriptors[resp_desc].flags = VIRTQ_DESC_F_WRITE;

        self.controlq.descriptors[cmd_desc].flags |= VIRTQ_DESC_F_NEXT;
        self.controlq.descriptors[cmd_desc].next = resp_desc as u16;

        self.controlq.submit(cmd_desc);
        self.transport.notify_queue(0);

        let mut retries = 1000;
        while !self.controlq.has_used() && retries > 0 {
            retries -= 1;
            core::hint::spin_loop();
        }

        if let Some((id, len)) = self.controlq.pop_used() {
            self.controlq.free_desc(id as usize);
            if len > 0 { Ok(()) } else { Err(VirtioGpuError::CommandFailed(0)) }
        } else {
            Err(VirtioGpuError::Timeout)
        }
    }

    pub fn get_display_info(&mut self) -> Result<DisplayInfoResponse, VirtioGpuError> {
        let hdr = GpuCtrlHdr::new(GpuCmdType::GetDisplayInfo);
        let mut resp = DisplayInfoResponse {
            header: GpuCtrlHdr { ctrl_type: GpuCmdType::RespOkDisplayInfo as u32, flags: 0, fence_id: 0, ctx_id: 0, padding: 0 },
            pmodes: [DisplayOne::default(); 16],
        };
        // Safety: resp is a packed repr(C) struct used as a DMA buffer for the device response
        self.send_command(&hdr, unsafe {
            core::slice::from_raw_parts_mut(&mut resp as *mut DisplayInfoResponse as *mut u8, core::mem::size_of::<DisplayInfoResponse>())
        })?;
        if resp.pmodes[0].xres > 0 && resp.pmodes[0].yres > 0 {
            self.display_width = resp.pmodes[0].xres;
            self.display_height = resp.pmodes[0].yres;
        }
        Ok(resp)
    }

    pub fn create_resource_2d(&mut self, width: u32, height: u32, format: u32) -> Result<u32, VirtioGpuError> {
        let resource_id = self.next_resource_id;
        self.next_resource_id += 1;
        let cmd = ResourceCreate2d { header: GpuCtrlHdr::new(GpuCmdType::ResourceCreate2d), resource_id, format, width, height };
        let mut resp = [0u8; 24];
        self.send_command(&cmd, &mut resp)?;
        self.resources.push(GpuResource { id: resource_id, width, height, format, backing_addr: None, backing_size: 0 });
        Ok(resource_id)
    }

    pub fn attach_backing(&mut self, resource_id: u32, addr: u64, size: u64) -> Result<(), VirtioGpuError> {
        let _resource = self.resources.iter().find(|r| r.id == resource_id)
            .ok_or(VirtioGpuError::InvalidResource)?;
        let cmd = ResourceAttachBacking { header: GpuCtrlHdr::new(GpuCmdType::ResourceAttachBacking), resource_id, nr_entries: 1 };
        let mut resp = [0u8; 24];
        self.send_command(&cmd, &mut resp)?;
        if let Some(r) = self.resources.iter_mut().find(|r| r.id == resource_id) {
            r.backing_addr = Some(addr);
            r.backing_size = size;
        }
        Ok(())
    }

    pub fn detach_backing(&mut self, resource_id: u32) -> Result<(), VirtioGpuError> {
        let _resource = self.resources.iter().find(|r| r.id == resource_id)
            .ok_or(VirtioGpuError::InvalidResource)?;
        let cmd = ResourceAttachBacking { header: GpuCtrlHdr::new(GpuCmdType::ResourceDetachBacking), resource_id, nr_entries: 0 };
        let mut resp = [0u8; 24];
        self.send_command(&cmd, &mut resp)?;
        if let Some(r) = self.resources.iter_mut().find(|r| r.id == resource_id) {
            r.backing_addr = None;
            r.backing_size = 0;
        }
        Ok(())
    }

    pub fn set_scanout(&mut self, scanout_id: u32, resource_id: u32, x: u32, y: u32, width: u32, height: u32) -> Result<(), VirtioGpuError> {
        let _resource = self.resources.iter().find(|r| r.id == resource_id)
            .ok_or(VirtioGpuError::InvalidResource)?;
        let cmd = SetScanout { header: GpuCtrlHdr::new(GpuCmdType::SetScanout), scanout_id, resource_id, x, y, width, height };
        let mut resp = [0u8; 24];
        self.send_command(&cmd, &mut resp)?;
        self.scanout_resource = Some(resource_id);
        Ok(())
    }

    pub fn transfer_to_host_2d(&mut self, resource_id: u32, x: u32, y: u32, width: u32, height: u32) -> Result<(), VirtioGpuError> {
        let _resource = self.resources.iter().find(|r| r.id == resource_id)
            .ok_or(VirtioGpuError::InvalidResource)?;
        let cmd = TransferToHost2d { header: GpuCtrlHdr::new(GpuCmdType::TransferToHost2d), resource_id, x, y, width, height };
        let mut resp = [0u8; 24];
        self.send_command(&cmd, &mut resp)
    }

    pub fn resource_flush_2d(&mut self, resource_id: u32, x: u32, y: u32, width: u32, height: u32) -> Result<(), VirtioGpuError> {
        let _resource = self.resources.iter().find(|r| r.id == resource_id)
            .ok_or(VirtioGpuError::InvalidResource)?;
        let cmd = ResourceFlush2d { header: GpuCtrlHdr::new(GpuCmdType::ResourceFlush2d), resource_id, x, y, width, height };
        let mut resp = [0u8; 24];
        self.send_command(&cmd, &mut resp)
    }

    pub fn destroy_resource(&mut self, resource_id: u32) -> Result<(), VirtioGpuError> {
        let len = self.resources.len();
        self.resources.retain(|r| r.id != resource_id);
        if self.resources.len() == len {
            return Err(VirtioGpuError::InvalidResource);
        }
        if self.scanout_resource == Some(resource_id) {
            self.scanout_resource = None;
        }
        Ok(())
    }

    pub fn display_info(&self) -> (u32, u32) {
        (self.display_width, self.display_height)
    }

    pub fn resources(&self) -> &[GpuResource] {
        &self.resources
    }

    pub fn set_flip_complete_flag(&mut self, flag: alloc::sync::Arc<AtomicBool>) {
        self.flip_complete = Some(flag);
    }

    pub fn page_flip(&mut self) -> Result<(), VirtioGpuError> {
        let resource_id = self.scanout_resource.ok_or(VirtioGpuError::InvalidScanout)?;
        self.transfer_to_host_2d(resource_id, 0, 0, self.display_width, self.display_height)?;
        self.resource_flush_2d(resource_id, 0, 0, self.display_width, self.display_height)?;
        if let Some(flag) = &self.flip_complete {
            flag.store(true, Ordering::Release);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::transport::VirtioTransport;

    struct MockTransport {
        status: u8,
        features: u64,
        config: [u8; 256],
        isr_status: u8,
    }

    impl MockTransport {
        fn new() -> Self {
            Self { status: 0, features: 0x1, config: [0u8; 256], isr_status: 0 }
        }
    }

    impl VirtioTransport for MockTransport {
        fn device_type(&self) -> u32 { 16 }
        fn get_status(&self) -> u8 { self.status }
        fn set_status(&mut self, status: u8) { self.status = status; }
        fn get_features(&self) -> u64 { self.features }
        fn set_features(&mut self, features: u64) { self.features = features; }
        fn config_read_u8(&self, offset: usize) -> u8 { self.config.get(offset).copied().unwrap_or(0) }
        fn config_write_u8(&mut self, offset: usize, value: u8) {
            if offset < self.config.len() { self.config[offset] = value; }
        }
        fn num_queues(&self) -> u16 { 2 }
        fn setup_queue(&mut self, _idx: u16, _addr: u64, _size: u16) -> Result<(), TransportError> { Ok(()) }
        fn notify_queue(&self, _idx: u16) {}
        fn get_isr_status(&self) -> u8 { self.isr_status }
    }

    #[test]
    fn test_gpu_driver_initialization() {
        let _guard = crate::test_serial::acquire();
        let transport = MockTransport::new();
        let mut driver = VirtioGpuHardwareDriver::new(Box::new(transport));
        assert!(driver.initialize().is_ok());
        assert!(driver.initialized);
    }

    #[test]
    fn test_gpu_driver_not_initialized() {
        let _guard = crate::test_serial::acquire();
        let transport = MockTransport::new();
        let mut driver = VirtioGpuHardwareDriver::new(Box::new(transport));
        // Should fail when not initialized
        assert_eq!(driver.create_resource_2d(100, 100, 0), Err(VirtioGpuError::DeviceFailed));
    }

    #[test]
    fn test_gpu_display_info_default() {
        let _guard = crate::test_serial::acquire();
        let transport = MockTransport::new();
        let driver = VirtioGpuHardwareDriver::new(Box::new(transport));
        // Display info should be 0,0 before initialization
        assert_eq!(driver.display_info(), (0, 0));
    }

    #[test]
    fn test_gpu_resources_empty() {
        let _guard = crate::test_serial::acquire();
        let transport = MockTransport::new();
        let driver = VirtioGpuHardwareDriver::new(Box::new(transport));
        assert_eq!(driver.resources().len(), 0);
    }

    #[test]
    fn test_gpu_resource_tracking() {
        let _guard = crate::test_serial::acquire();
        let transport = MockTransport::new();
        let mut driver = VirtioGpuHardwareDriver::new(Box::new(transport));
        driver.initialized = true;

        // Manually add a resource to test tracking
        driver.resources.push(GpuResource {
            id: 1,
            width: 1920,
            height: 1080,
            format: formats::DRM_FORMAT_XRGB8888,
            backing_addr: None,
            backing_size: 0,
        });

        assert_eq!(driver.resources().len(), 1);
        assert_eq!(driver.resources()[0].width, 1920);
    }

    #[test]
    fn test_gpu_destroy_nonexistent_resource() {
        let _guard = crate::test_serial::acquire();
        let transport = MockTransport::new();
        let mut driver = VirtioGpuHardwareDriver::new(Box::new(transport));
        driver.initialized = true;

        assert_eq!(driver.destroy_resource(999), Err(VirtioGpuError::InvalidResource));
    }

    #[test]
    fn test_gpu_set_scanout_invalid_resource() {
        let _guard = crate::test_serial::acquire();
        let transport = MockTransport::new();
        let mut driver = VirtioGpuHardwareDriver::new(Box::new(transport));
        driver.initialized = true;

        assert_eq!(
            driver.set_scanout(0, 999, 0, 0, 100, 100),
            Err(VirtioGpuError::InvalidResource)
        );
    }

    #[test]
    fn test_gpu_page_flip_no_scanout() {
        let _guard = crate::test_serial::acquire();
        let transport = MockTransport::new();
        let mut driver = VirtioGpuHardwareDriver::new(Box::new(transport));
        driver.initialized = true;

        assert_eq!(driver.page_flip(), Err(VirtioGpuError::InvalidScanout));
    }
}
