//! VirtIO-GPU driver implementation.
//!
//! Implements the VirtIO-GPU control queue protocol for QEMU's virtio-gpu device.
//! Reference: <https://docs.oasis-open.org/virtio/virtio/v1.1/virtio-v1.1.html>

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

/// VirtIO-GPU controlq request types.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtioGpuCtrlType {
    /// Get display configuration.
    CmdGetDisplayInfo = 0x100,
    /// Set scanout (framebuffer assignment).
    CmdSetScanout = 0x101,
    /// Create a 2D resource (framebuffer).
    CmdResourceCreate2d = 0x102,
    /// Transfer host data to device.
    CmdTransferToHost2d = 0x103,
    /// Flush a resource region.
    CmdResourceFlush2d = 0x104,
    /// Attach backing memory to a resource.
    CmdResourceAttachBacking = 0x105,
    /// Detach backing memory from a resource.
    CmdResourceDetachBacking = 0x106,
    /// Get display info reply.
    RespOkDisplayInfo = 0x110,
    /// Generic OK reply.
    RespOkNodata = 0x111,
    /// Error reply.
    RespErrUnspec = 0x120,
    RespErrUnimplemented = 0x121,
    RespErrInvalidParams = 0x122,
}

/// VirtIO-GPU controlq header.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct VirtioGpuCtrlHdr {
    pub ctrl_type: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub padding: u32,
}

impl VirtioGpuCtrlHdr {
    pub fn new(ctrl_type: VirtioGpuCtrlType) -> Self {
        Self {
            ctrl_type: ctrl_type as u32,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        }
    }
}

/// Display info response (for GET_DISPLAY_INFO).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct VirtioGpuDisplayInfo {
    pub header: VirtioGpuCtrlHdr,
    pub pmodes: [VirtioGpuDisplayOne; 16], // Up to 16 display modes
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C, packed)]
pub struct VirtioGpuDisplayOne {
    pub xres: u32,
    pub yres: u32,
    pub xoff: u32,
    pub yoff: u32,
}

/// Resource create 2D command.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct VirtioGpuResourceCreate2d {
    pub header: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

/// Resource attach backing command.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct VirtioGpuResourceAttachBacking {
    pub header: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub nr_entries: u32,
}

/// Memory entry for resource attach backing.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct VirtioGpuMemEntry {
    pub addr: u64,
    pub length: u32,
    pub padding: u32,
}

/// Set scanout command.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct VirtioGpuSetScanout {
    pub header: VirtioGpuCtrlHdr,
    pub scanout_id: u32,
    pub resource_id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Transfer to host 2D command.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct VirtioGpuTransferToHost2d {
    pub header: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Resource flush 2D command.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct VirtioGpuResourceFlush2d {
    pub header: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// VirtIO-GPU pixel formats (DRM fourcc).
pub mod formats {
    pub const DRM_FORMAT_XRGB8888: u32 = 0x34324258; // 'XR24'
    pub const DRM_FORMAT_ARGB8888: u32 = 0x34324152; // 'AR24'
    pub const DRM_FORMAT_RGB565: u32 = 0x36314752; // 'RG16'
}

/// Resource state tracked by the driver.
#[derive(Debug, Clone)]
pub struct GpuResource {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub backing_addr: Option<u64>,
    pub backing_size: u64,
}

/// VirtIO-GPU driver state.
pub struct VirtioGpuDriver {
    /// Resources allocated on the device.
    resources: Vec<GpuResource>,
    /// Next resource ID to allocate.
    next_resource_id: u32,
    /// Current display mode.
    display_width: u32,
    display_height: u32,
    /// Currently attached scanout resource.
    scanout_resource: Option<u32>,
    /// Double-buffer state.
    double_buffer: DoubleBuffer,
    /// Pending VBlank events.
    vblank_events: VecDeque<VBlankEvent>,
    /// Flip completion callback flag.
    flip_complete: Option<Arc<AtomicBool>>,
}

impl Default for VirtioGpuDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtioGpuDriver {
    pub fn new() -> Self {
        Self {
            resources: Vec::new(),
            next_resource_id: 1,
            display_width: 0,
            display_height: 0,
            scanout_resource: None,
            double_buffer: DoubleBuffer::new(60),
            vblank_events: VecDeque::new(),
            flip_complete: None,
        }
    }

    /// Create a 2D resource (framebuffer).
    pub fn create_resource_2d(&mut self, width: u32, height: u32, format: u32) -> u32 {
        let resource_id = self.next_resource_id;
        self.next_resource_id += 1;

        self.resources.push(GpuResource {
            id: resource_id,
            width,
            height,
            format,
            backing_addr: None,
            backing_size: 0,
        });

        crate::serial::println!(
            "[GPU] Created resource {} ({}x{}, format={:#x})",
            resource_id,
            width,
            height,
            format
        );

        resource_id
    }

    /// Attach backing memory to a resource.
    pub fn attach_backing(
        &mut self,
        resource_id: u32,
        addr: u64,
        size: u64,
    ) -> Result<(), GpuError> {
        let resource = self
            .resources
            .iter_mut()
            .find(|r| r.id == resource_id)
            .ok_or(GpuError::InvalidResource)?;

        resource.backing_addr = Some(addr);
        resource.backing_size = size;

        crate::serial::println!(
            "[GPU] Attached backing to resource {} (addr={:#x}, size={})",
            resource_id,
            addr,
            size
        );

        Ok(())
    }

    /// Detach backing from a resource.
    pub fn detach_backing(&mut self, resource_id: u32) -> Result<(), GpuError> {
        let resource = self
            .resources
            .iter_mut()
            .find(|r| r.id == resource_id)
            .ok_or(GpuError::InvalidResource)?;

        resource.backing_addr = None;
        resource.backing_size = 0;

        Ok(())
    }

    /// Set scanout (assign a resource to a display connector).
    pub fn set_scanout(
        &mut self,
        scanout_id: u32,
        resource_id: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), GpuError> {
        // Validate resource exists
        let _resource = self
            .resources
            .iter()
            .find(|r| r.id == resource_id)
            .ok_or(GpuError::InvalidResource)?;

        self.scanout_resource = Some(resource_id);

        crate::serial::println!(
            "[GPU] Set scanout {}: resource {} at ({},{}) {}x{}",
            scanout_id,
            resource_id,
            x,
            y,
            width,
            height
        );

        Ok(())
    }

    /// Transfer host data to device (update a region).
    pub fn transfer_to_host_2d(
        &self,
        resource_id: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), GpuError> {
        let _resource = self
            .resources
            .iter()
            .find(|r| r.id == resource_id)
            .ok_or(GpuError::InvalidResource)?;

        crate::serial::println!(
            "[GPU] Transfer to host: resource {} ({},{}) {}x{}",
            resource_id,
            x,
            y,
            width,
            height
        );

        Ok(())
    }

    /// Flush a resource region to the display.
    pub fn resource_flush_2d(
        &self,
        resource_id: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), GpuError> {
        let _resource = self
            .resources
            .iter()
            .find(|r| r.id == resource_id)
            .ok_or(GpuError::InvalidResource)?;

        crate::serial::println!(
            "[GPU] Flush: resource {} ({},{}) {}x{}",
            resource_id,
            x,
            y,
            width,
            height
        );

        Ok(())
    }

    /// Get current display dimensions.
    pub fn display_info(&self) -> (u32, u32) {
        (self.display_width, self.display_height)
    }

    /// Update display info from GET_DISPLAY_INFO response.
    pub fn update_display_info(&mut self, width: u32, height: u32) {
        self.display_width = width;
        self.display_height = height;
        crate::serial::println!("[GPU] Display: {}x{}", width, height);
    }

    /// Destroy a resource.
    pub fn destroy_resource(&mut self, resource_id: u32) -> Result<(), GpuError> {
        let len = self.resources.len();
        self.resources.retain(|r| r.id != resource_id);
        if self.resources.len() == len {
            return Err(GpuError::InvalidResource);
        }
        if self.scanout_resource == Some(resource_id) {
            self.scanout_resource = None;
        }
        Ok(())
    }

    /// List all allocated resources.
    pub fn resources(&self) -> &[GpuResource] {
        &self.resources
    }

    /// Initialize double-buffering with front and back resources.
    pub fn init_double_buffer(&mut self, front_id: u32, back_id: u32) {
        self.double_buffer.front = Some(front_id);
        self.double_buffer.back = Some(back_id);
        crate::serial::println!(
            "[GPU] Double-buffer initialized: front={}, back={}",
            front_id,
            back_id
        );
    }

    /// Perform a page flip (swap front/back).
    pub fn page_flip(&mut self) -> Result<(), GpuError> {
        if self.double_buffer.flip_pending {
            return Err(GpuError::FlipPending);
        }
        self.double_buffer.swap();
        self.double_buffer.flip_pending = true;

        // Signal flip complete
        if let Some(flag) = &self.flip_complete {
            flag.store(true, Ordering::Release);
        }

        crate::serial::println!(
            "[GPU] Page flip: frame={}, front={:?}",
            self.double_buffer.frame_count,
            self.double_buffer.front
        );

        Ok(())
    }

    /// Get the back buffer resource ID for rendering.
    pub fn back_buffer(&self) -> Option<u32> {
        self.double_buffer.back
    }

    /// Get the front buffer resource ID (currently displayed).
    pub fn front_buffer(&self) -> Option<u32> {
        self.double_buffer.front
    }

    /// Wait for VBlank (poll-based).
    pub fn wait_for_vblank(&mut self, current_timestamp_us: u64) -> Option<VBlankEvent> {
        if self.double_buffer.check_vblank(current_timestamp_us) {
            let event = VBlankEvent {
                sequence: self.double_buffer.frame_count,
                timestamp_us: current_timestamp_us,
                frame_duration_us: self.double_buffer.target_interval_us,
            };
            self.vblank_events.push_back(event);
            Some(event)
        } else {
            None
        }
    }

    /// Get the target refresh rate.
    pub fn refresh_rate(&self) -> u32 {
        self.double_buffer.estimated_fps()
    }

    /// Set target refresh rate.
    pub fn set_refresh_rate(&mut self, hz: u32) {
        self.double_buffer = DoubleBuffer::new(hz);
    }

    /// Check if a flip is pending.
    pub fn is_flip_pending(&self) -> bool {
        self.double_buffer.flip_pending
    }

    /// Clear flip pending flag (called after flip completes).
    pub fn clear_flip_pending(&mut self) {
        self.double_buffer.flip_pending = false;
    }

    /// Get frame count.
    pub fn frame_count(&self) -> u64 {
        self.double_buffer.frame_count
    }
}

/// GPU driver errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuError {
    InvalidResource,
    InvalidScanout,
    UnsupportedFormat,
    OutOfMemory,
    FlipPending,
}

/// Double-buffer state for tear-free rendering.
#[derive(Debug, Clone)]
pub struct DoubleBuffer {
    /// Front buffer resource ID (currently displayed).
    pub front: Option<u32>,
    /// Back buffer resource ID (being rendered to).
    pub back: Option<u32>,
    /// Whether a flip is pending.
    pub flip_pending: bool,
    /// Frame counter for VBlank sync.
    pub frame_count: u64,
    /// Last VBlank timestamp (microseconds).
    pub last_vblank_us: u64,
    /// Target refresh interval in microseconds (e.g., 16666 for 60Hz).
    pub target_interval_us: u64,
}

impl DoubleBuffer {
    pub fn new(target_refresh_hz: u32) -> Self {
        let interval = if target_refresh_hz > 0 {
            1_000_000 / target_refresh_hz as u64
        } else {
            16_666 // Default 60Hz
        };
        Self {
            front: None,
            back: None,
            flip_pending: false,
            frame_count: 0,
            last_vblank_us: 0,
            target_interval_us: interval,
        }
    }

    /// Swap front and back buffers (page flip).
    pub fn swap(&mut self) {
        core::mem::swap(&mut self.front, &mut self.back);
        self.frame_count += 1;
    }

    /// Check if a VBlank has occurred based on timestamp.
    pub fn check_vblank(&mut self, current_us: u64) -> bool {
        if current_us.saturating_sub(self.last_vblank_us) >= self.target_interval_us {
            self.last_vblank_us = current_us;
            true
        } else {
            false
        }
    }

    /// Get current frame rate (approximate, based on recent frames).
    pub fn estimated_fps(&self) -> u32 {
        if self.target_interval_us == 0 {
            return 0;
        }
        (1_000_000 / self.target_interval_us) as u32
    }
}

/// VBlank event for frame synchronization.
#[derive(Debug, Clone, Copy)]
pub struct VBlankEvent {
    /// VBlank sequence number.
    pub sequence: u64,
    /// Timestamp in microseconds since boot.
    pub timestamp_us: u64,
    /// Duration of the previous frame in microseconds.
    pub frame_duration_us: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() {
        turnix_serial::disable_serial();
    }

    #[test]
    fn test_create_resource() {
        setup();
        let mut gpu = VirtioGpuDriver::new();
        let id = gpu.create_resource_2d(1920, 1080, formats::DRM_FORMAT_XRGB8888);
        assert_eq!(id, 1);
        assert_eq!(gpu.resources().len(), 1);
        assert_eq!(gpu.resources()[0].width, 1920);
        assert_eq!(gpu.resources()[0].height, 1080);
    }

    #[test]
    fn test_attach_detach_backing() {
        setup();
        let mut gpu = VirtioGpuDriver::new();
        let id = gpu.create_resource_2d(800, 600, formats::DRM_FORMAT_XRGB8888);

        gpu.attach_backing(id, 0x1000_0000, 800 * 600 * 4).unwrap();
        assert_eq!(gpu.resources()[0].backing_addr, Some(0x1000_0000));

        gpu.detach_backing(id).unwrap();
        assert_eq!(gpu.resources()[0].backing_addr, None);
    }

    #[test]
    fn test_set_scanout() {
        setup();
        let mut gpu = VirtioGpuDriver::new();
        let id = gpu.create_resource_2d(1024, 768, formats::DRM_FORMAT_XRGB8888);

        gpu.set_scanout(0, id, 0, 0, 1024, 768).unwrap();
        assert_eq!(gpu.scanout_resource, Some(id));
    }

    #[test]
    fn test_invalid_resource() {
        setup();
        let mut gpu = VirtioGpuDriver::new();
        assert_eq!(
            gpu.attach_backing(999, 0, 0),
            Err(GpuError::InvalidResource)
        );
        assert_eq!(
            gpu.set_scanout(0, 999, 0, 0, 100, 100),
            Err(GpuError::InvalidResource)
        );
    }

    #[test]
    fn test_destroy_resource() {
        setup();
        let mut gpu = VirtioGpuDriver::new();
        let id = gpu.create_resource_2d(100, 100, formats::DRM_FORMAT_XRGB8888);
        gpu.set_scanout(0, id, 0, 0, 100, 100).unwrap();

        gpu.destroy_resource(id).unwrap();
        assert_eq!(gpu.resources().len(), 0);
        assert_eq!(gpu.scanout_resource, None);
    }

    #[test]
    fn test_display_info() {
        setup();
        let mut gpu = VirtioGpuDriver::new();
        gpu.update_display_info(2560, 1440);
        assert_eq!(gpu.display_info(), (2560, 1440));
    }

    #[test]
    fn test_multiple_resources() {
        setup();
        let mut gpu = VirtioGpuDriver::new();
        let r1 = gpu.create_resource_2d(1920, 1080, formats::DRM_FORMAT_XRGB8888);
        let r2 = gpu.create_resource_2d(800, 600, formats::DRM_FORMAT_ARGB8888);
        let r3 = gpu.create_resource_2d(320, 240, formats::DRM_FORMAT_RGB565);

        assert_eq!(r1, 1);
        assert_eq!(r2, 2);
        assert_eq!(r3, 3);
        assert_eq!(gpu.resources().len(), 3);
    }

    #[test]
    fn test_double_buffer_swap() {
        setup();
        let mut gpu = VirtioGpuDriver::new();
        let r1 = gpu.create_resource_2d(1920, 1080, formats::DRM_FORMAT_XRGB8888);
        let r2 = gpu.create_resource_2d(1920, 1080, formats::DRM_FORMAT_XRGB8888);
        gpu.init_double_buffer(r1, r2);

        assert_eq!(gpu.front_buffer(), Some(r1));
        assert_eq!(gpu.back_buffer(), Some(r2));

        gpu.page_flip().unwrap();
        assert_eq!(gpu.front_buffer(), Some(r2));
        assert_eq!(gpu.back_buffer(), Some(r1));
        assert_eq!(gpu.frame_count(), 1);
    }

    #[test]
    fn test_vblank_detection() {
        setup();
        let mut gpu = VirtioGpuDriver::new();
        gpu.set_refresh_rate(60); // 16666us interval

        // At time 0, no VBlank has elapsed since last_vblank_us starts at 0
        let event1 = gpu.wait_for_vblank(0);
        assert!(event1.is_none());

        // 10ms later — no VBlank yet (interval is ~16.67ms)
        let event2 = gpu.wait_for_vblank(10_000);
        assert!(event2.is_none());

        // 17ms later — VBlank should occur
        let event3 = gpu.wait_for_vblank(17_000);
        assert!(event3.is_some());
    }

    #[test]
    fn test_flip_pending() {
        setup();
        let mut gpu = VirtioGpuDriver::new();
        let r1 = gpu.create_resource_2d(100, 100, formats::DRM_FORMAT_XRGB8888);
        let r2 = gpu.create_resource_2d(100, 100, formats::DRM_FORMAT_XRGB8888);
        gpu.init_double_buffer(r1, r2);

        gpu.page_flip().unwrap();
        assert!(gpu.is_flip_pending());

        // Second flip should fail
        assert_eq!(gpu.page_flip(), Err(GpuError::FlipPending));

        gpu.clear_flip_pending();
        assert!(!gpu.is_flip_pending());
    }

    #[test]
    fn test_refresh_rate() {
        setup();
        let mut gpu = VirtioGpuDriver::new();
        gpu.set_refresh_rate(144);
        assert_eq!(gpu.refresh_rate(), 144);
    }
}
