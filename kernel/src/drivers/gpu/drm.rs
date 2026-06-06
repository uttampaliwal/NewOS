//! Concrete GPU/DRM display driver implementations.
//!
//! Supported devices:
//! - **Bochs-display** (QEMU `-device bochs-display`): VBE DISPI extended
//!   interface via I/O ports `0x1CE`/`0x1CF`, framebuffer on PCI BAR2.
//! - **Virtio-gpu** (QEMU `-device virtio-gpu`): stub that falls back to the
//!   UEFI framebuffer.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Debug;

use super::{Connector, ConnectorType, DisplayMode, DrmDevice, DrmError, DrmFramebuffer};
use crate::drivers::framework::DeviceInfo;

// ---------------------------------------------------------------------------
// VBE DISPI constants for bochs-display
// ---------------------------------------------------------------------------

/// VBE DISPI index I/O port.
#[allow(dead_code)]
const VBE_DISPI_IOPORT_INDEX: u16 = 0x1CE;
/// VBE DISPI data I/O port.
#[allow(dead_code)]
const VBE_DISPI_IOPORT_DATA: u16 = 0x1CF;

/// VBE DISPI register indices.
const VBE_DISPI_INDEX_ID: u16 = 0x00;
const VBE_DISPI_INDEX_XRES: u16 = 0x01;
const VBE_DISPI_INDEX_YRES: u16 = 0x02;
const VBE_DISPI_INDEX_BPP: u16 = 0x03;
const VBE_DISPI_INDEX_ENABLE: u16 = 0x04;
#[allow(dead_code)]
const VBE_DISPI_INDEX_BANK: u16 = 0x05;
const VBE_DISPI_INDEX_VIRT_WIDTH: u16 = 0x06;
const VBE_DISPI_INDEX_VIRT_HEIGHT: u16 = 0x07;
const VBE_DISPI_INDEX_X_OFFSET: u16 = 0x08;
const VBE_DISPI_INDEX_Y_OFFSET: u16 = 0x09;

/// VBE DISPI enable flags.
const VBE_DISPI_DISABLED: u16 = 0x00;
const VBE_DISPI_ENABLED: u16 = 0x01;
const VBE_DISPI_LFB_ENABLED: u16 = 0x40;

/// Maximum supported VBE DISPI version.
const VBE_DISPI_VERSION_MIN: u16 = 0xB0C0;
const VBE_DISPI_VERSION_MAX: u16 = 0xB0C5;

// ---------------------------------------------------------------------------
// BochsDisplayDriver
// ---------------------------------------------------------------------------

/// Driver for QEMU's bochs-display device.
///
/// The device emulates a VBE-compatible display controller exposed via the
/// VBE DISPI extended interface (I/O ports `0x1CE`/`0x1CF`). The linear
/// framebuffer is mapped on PCI BAR2.
///
/// # Hardware initialisation
/// 1. Probe VBE DISPI by reading the ID register.
/// 2. Negotiate `1920×1080×32bpp` mode.
/// 3. Enable LFB (Linear Framebuffer) at the BAR2 address.
#[derive(Debug)]
pub struct BochsDisplayDriver {
    /// Framebuffer physical address (from PCI BAR2).
    fb_addr: u64,
    /// Framebuffer BAR size (total size available).
    fb_bar_size: u64,
    /// Currently set display mode.
    current_mode: Option<DisplayMode>,
    /// Whether the VBE DISPI interface was successfully probed.
    vbe_available: bool,
    /// Vendor device IDs for logging.
    #[allow(dead_code)]
    vendor_id: u16,
    #[allow(dead_code)]
    device_id: u16,
}

impl BochsDisplayDriver {
    /// Create a new driver instance for a bochs-display device.
    ///
    /// The framebuffer address is always at BAR2; BAR0 contains MMIO registers
    /// in newer bochs-display revisions, but we prefer the VBE DISPI I/O port
    /// interface for mode setting.
    pub fn new(info: &DeviceInfo) -> Self {
        // Extract framebuffer base from BAR2 (index 2).
        let fb_addr = match &info.bars[2] {
            Some(crate::drivers::framework::Bar::Memory32 { base, .. }) => *base as u64,
            Some(crate::drivers::framework::Bar::Memory64 { base, .. }) => *base,
            _ => 0,
        };
        let fb_size = match &info.bars[2] {
            Some(crate::drivers::framework::Bar::Memory32 { size, .. }) => *size as u64,
            Some(crate::drivers::framework::Bar::Memory64 { size, .. }) => *size,
            _ => 0,
        };

        // Probe VBE DISPI availability.
        let vbe_available = Self::probe_vbe();

        Self {
            fb_addr,
            fb_bar_size: fb_size,
            current_mode: None,
            vbe_available,
            vendor_id: info.vendor_id,
            device_id: info.device_id,
        }
    }

    /// Check whether VBE DISPI is available and meets version requirements.
    fn probe_vbe() -> bool {
        if cfg!(test) {
            return false;
        }
        // SAFETY: only called during kernel boot when VBE I/O ports are accessible.
        let id = unsafe { Self::vbe_read_index(VBE_DISPI_INDEX_ID) };
        (VBE_DISPI_VERSION_MIN..=VBE_DISPI_VERSION_MAX).contains(&id)
    }

    /// Write to a VBE DISPI index register.
    ///
    /// # Safety
    /// Accesses I/O ports `0x1CE`/`0x1CF`.
    unsafe fn vbe_write_index(index: u16, value: u16) {
        #[cfg(not(test))]
        // SAFETY: caller must ensure I/O ports are accessible
        unsafe {
            core::arch::asm!(
                "outw %ax, %dx",
                in("dx") VBE_DISPI_IOPORT_INDEX,
                in("ax") index,
                options(att_syntax, nostack)
            );
            core::arch::asm!(
                "outw %ax, %dx",
                in("dx") VBE_DISPI_IOPORT_DATA,
                in("ax") value,
                options(att_syntax, nostack)
            );
        }
        #[cfg(test)]
        {
            let _ = (index, value);
        }
    }

    /// Read from a VBE DISPI data register.
    ///
    /// # Safety
    /// Accesses I/O ports `0x1CE`/`0x1CF`.
    unsafe fn vbe_read_index(index: u16) -> u16 {
        #[cfg(not(test))]
        // SAFETY: caller must ensure I/O ports are accessible
        unsafe {
            // Write index
            core::arch::asm!(
                "outw %ax, %dx",
                in("dx") VBE_DISPI_IOPORT_INDEX,
                in("ax") index,
                options(att_syntax, nostack)
            );
            // Read value
            let value: u16;
            core::arch::asm!(
                "inw %dx, %ax",
                in("dx") VBE_DISPI_IOPORT_DATA,
                out("ax") value,
                options(att_syntax, nostack)
            );
            value
        }
        #[cfg(test)]
        {
            let _ = index;
            0
        }
    }

    /// Set a VBE display mode.
    ///
    /// Disables the controller, writes resolution and BPP, then re-enables
    /// with the LFB flag.
    unsafe fn vbe_set_mode(&self, width: u16, height: u16, bpp: u16) {
        // SAFETY: caller ensures I/O ports are accessible
        unsafe {
            Self::vbe_write_index(VBE_DISPI_INDEX_ENABLE, VBE_DISPI_DISABLED);
            Self::vbe_write_index(VBE_DISPI_INDEX_XRES, width);
            Self::vbe_write_index(VBE_DISPI_INDEX_YRES, height);
            Self::vbe_write_index(VBE_DISPI_INDEX_BPP, bpp);
            Self::vbe_write_index(VBE_DISPI_INDEX_VIRT_WIDTH, width);
            Self::vbe_write_index(VBE_DISPI_INDEX_VIRT_HEIGHT, height);
            Self::vbe_write_index(VBE_DISPI_INDEX_X_OFFSET, 0);
            Self::vbe_write_index(VBE_DISPI_INDEX_Y_OFFSET, 0);
            Self::vbe_write_index(
                VBE_DISPI_INDEX_ENABLE,
                VBE_DISPI_ENABLED | VBE_DISPI_LFB_ENABLED,
            );
        }
    }
}

impl DrmDevice for BochsDisplayDriver {
    /// Bochs-display has one fixed virtual connector.
    fn enumerate_connectors(&self) -> Result<Vec<Connector>, DrmError> {
        Ok(vec![Connector {
            id: 1,
            connector_type: ConnectorType::DisplayPort,
            connected: true,
            modes: vec![
                DisplayMode::new(1920, 1080),
                DisplayMode::new(1280, 720),
                DisplayMode::new(1024, 768),
                DisplayMode::new(800, 600),
                DisplayMode::new(640, 480),
            ],
        }])
    }

    fn set_mode(
        &mut self,
        _connector_id: u32,
        _crtc_id: u32,
        mode: &DisplayMode,
    ) -> Result<(), DrmError> {
        if mode.width > 4096 || mode.height > 4096 {
            return Err(DrmError::InvalidMode);
        }

        // Write mode to VBE DISPI if available
        if self.vbe_available {
            unsafe {
                self.vbe_set_mode(mode.width as u16, mode.height as u16, 32);
            }
        }

        self.current_mode = Some(*mode);
        Ok(())
    }

    fn create_framebuffer(
        &mut self,
        width: u32,
        height: u32,
        _format: u32,
    ) -> Result<DrmFramebuffer, DrmError> {
        let needed = DrmFramebuffer::calc_size(width, height);
        // For bochs-display, the BAR2 is the framebuffer; we must fit.
        if needed > self.fb_bar_size && self.fb_bar_size > 0 {
            return Err(DrmError::InvalidFramebuffer);
        }
        Ok(DrmFramebuffer {
            id: 1,
            width,
            height,
            stride: width * 4,
            format: 0, // XRGB8888
            phys_addr: self.fb_addr,
            size: core::cmp::min(needed, self.fb_bar_size),
            active: true,
        })
    }

    fn page_flip(&mut self, _crtc_id: u32, _fb_id: u32) -> Result<(), DrmError> {
        // Single-buffered; page flip is a no-op.
        Ok(())
    }

    fn current_fb_addr(&self) -> u64 {
        self.fb_addr
    }

    fn current_fb_size(&self) -> u64 {
        if self.fb_bar_size > 0 {
            self.fb_bar_size
        } else {
            // Fallback to default framebuffer size (1920×1080×4)
            super::DEFAULT_FRAMEBUFFER_SIZE
        }
    }

    fn name(&self) -> &'static str {
        "bochs-display"
    }
}

// ---------------------------------------------------------------------------
// VirtioGpuDriver
// ---------------------------------------------------------------------------

/// Driver for QEMU's virtio-gpu device.
///
/// Virtio-gpu is a more capable device that supports 2D/3D acceleration and
/// multiple scan-out surfaces. This initial implementation provides a stub
/// that falls back to the UEFI framebuffer.
///
/// # Future improvements
/// - Implement virtio transport via the virtqueue.
/// - Support multiple connectors (e.g., GPU + DisplayPort).
/// - Implement 2D commands for accelerated rendering.
#[derive(Debug)]
pub struct VirtioGpuDriver {
    /// Framebuffer physical address (from UEFI GOP or PCI BAR).
    fb_addr: u64,
    /// Framebuffer size.
    fb_size: u64,
    /// Vendor and device IDs for logging.
    #[allow(dead_code)]
    vendor_id: u16,
    #[allow(dead_code)]
    device_id: u16,
}

impl DrmDevice for VirtioGpuDriver {
    /// Stub: report one connected connector with the default mode.
    fn enumerate_connectors(&self) -> Result<Vec<Connector>, DrmError> {
        Ok(vec![Connector {
            id: 1,
            connector_type: ConnectorType::DisplayPort,
            connected: true,
            modes: vec![
                DisplayMode::new(1920, 1080),
                DisplayMode::new(1280, 720),
                DisplayMode::new(1024, 768),
            ],
        }])
    }

    fn set_mode(
        &mut self,
        _connector_id: u32,
        _crtc_id: u32,
        _mode: &DisplayMode,
    ) -> Result<(), DrmError> {
        // Stub: virtio-gpu mode setting requires virtqueue commands.
        // Current implementation relies on UEFI GOP for initial mode.
        Ok(())
    }

    fn create_framebuffer(
        &mut self,
        _width: u32,
        _height: u32,
        _format: u32,
    ) -> Result<DrmFramebuffer, DrmError> {
        Ok(DrmFramebuffer {
            id: 1,
            width: 1920,
            height: 1080,
            stride: 1920 * 4,
            format: 0,
            phys_addr: self.fb_addr,
            size: self.fb_size,
            active: true,
        })
    }

    fn page_flip(&mut self, _crtc_id: u32, _fb_id: u32) -> Result<(), DrmError> {
        Ok(())
    }

    fn current_fb_addr(&self) -> u64 {
        self.fb_addr
    }

    fn current_fb_size(&self) -> u64 {
        self.fb_size
    }

    fn name(&self) -> &'static str {
        "virtio-gpu"
    }
}

// ---------------------------------------------------------------------------
// GPU probe dispatcher
// ---------------------------------------------------------------------------

/// Probe a PCI display controller and return a boxed DrmDevice.
///
/// # Device matching
/// - **Class `0x03`, Subclass `0x00`** → bochs-display (VBE DISPI)
/// - **Vendor `0x1AF4`, Device `0x1050`** → virtio-gpu
/// - All others → `Err(DrmError::NotSupported)`
///
/// # BAR layout for bochs-display
/// - BAR0: MMIO registers (not used — we prefer VBE DISPI I/O ports)
/// - BAR2: Linear framebuffer
pub fn probe_gpu(info: &DeviceInfo) -> Result<Box<dyn DrmDevice>, super::GpuError> {
    // Class 0x03 = Display controller
    if info.class_code != 0x03 {
        return Err(super::GpuError::ProbeFailed("not a display controller"));
    }

    match info.subclass {
        // VGA-compatible controller → bochs-display
        0x00 => {
            let mut driver = BochsDisplayDriver::new(info);
            if driver.vbe_available {
                // Initialise default 1920×1080 32bpp mode
                let mode = DisplayMode::new(super::DEFAULT_WIDTH, super::DEFAULT_HEIGHT);
                if driver.set_mode(1, 1, &mode).is_ok() {
                    crate::serial::println!(
                        "[GPU] bochs-display: {}x{} 32bpp mode set @ BAR2=0x{:016x}",
                        mode.width,
                        mode.height,
                        driver.fb_addr
                    );
                }
            } else {
                crate::serial::println!(
                    "[GPU] bochs-display: VBE DISPI unavailable, using EFI framebuffer @ 0x{:016x}",
                    driver.fb_addr
                );
            }
            Ok(Box::new(driver))
        }
        // XGA or 3D controller → try virtio-gpu
        _ => {
            if info.vendor_id == 0x1AF4 && info.device_id == 0x1050 {
                let drv = VirtioGpuDriver {
                    fb_addr: 0, // will be populated from boot_info.framebuffer
                    fb_size: 0,
                    vendor_id: info.vendor_id,
                    device_id: info.device_id,
                };
                crate::serial::println!(
                    "[GPU] virtio-gpu: stub driver (vendor=0x{:04x}, device=0x{:04x})",
                    info.vendor_id,
                    info.device_id
                );
                Ok(Box::new(drv))
            } else {
                Err(super::GpuError::ProbeFailed("unsupported display device"))
            }
        }
    }
}
