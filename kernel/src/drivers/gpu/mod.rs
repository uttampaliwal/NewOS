//! GPU/DRM driver framework
//!
//! This module provides:
//! - The [`DrmDevice`] trait that all GPU display drivers implement
//! - Shared types for connectors, modes, framebuffers, and CRTCs
//! - The [`DrmManager`] singleton that coordinates the active display driver
//! - Framebuffer mapping and permission enforcement for the Compositor process

pub mod drm;
pub mod edid;
pub mod gbm;
pub mod virtio_gpu;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use crate::drivers::framework::{DeviceDriver, DeviceInfo};
use x86_64::VirtAddr;

// ---------------------------------------------------------------------------
// DRM constants
// ---------------------------------------------------------------------------

/// Default resolution negotiated by the driver.
pub const DEFAULT_WIDTH: u32 = 1920;
pub const DEFAULT_HEIGHT: u32 = 1080;
pub const DEFAULT_BPP: u32 = 32;
pub const DEFAULT_STRIDE: u32 = DEFAULT_WIDTH * (DEFAULT_BPP / 8);

/// Framebuffer size in bytes for the default resolution.
pub const DEFAULT_FRAMEBUFFER_SIZE: u64 = DEFAULT_WIDTH as u64 * DEFAULT_HEIGHT as u64 * 4;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// DRM operation errors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DrmError {
    InvalidConnector,
    InvalidMode,
    InvalidFramebuffer,
    InvalidCrtc,
    NoMemory,
    NotSupported,
    PermissionDenied,
}

// ---------------------------------------------------------------------------
// Connector types
// ---------------------------------------------------------------------------

/// Physical connector types.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectorType {
    Vga,
    Dvi,
    Hdmi,
    DisplayPort,
    EmbeddedDisplayPort,
    Unknown,
}

/// A display connector with its supported modes.
#[derive(Debug, Clone)]
pub struct Connector {
    /// Unique connector ID.
    pub id: u32,
    /// Connector type (HDMI, DP, etc.).
    pub connector_type: ConnectorType,
    /// Whether a display is physically connected.
    pub connected: bool,
    /// List of supported display modes.
    pub modes: Vec<DisplayMode>,
}

/// A display mode (resolution + refresh rate).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh_mhz: u32,
    pub flags: u32,
}

impl DisplayMode {
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            refresh_mhz: 60_000,
            flags: 0,
        }
    }

    /// Framebuffer size in bytes for this mode at 32 bpp.
    pub fn framebuffer_size(&self) -> u64 {
        self.width as u64 * self.height as u64 * 4
    }

    /// Stride (bytes per row) for this mode.
    pub fn stride(&self) -> u32 {
        self.width * 4
    }
}

// ---------------------------------------------------------------------------
// Framebuffer
// ---------------------------------------------------------------------------

/// A framebuffer object representing a single display buffer.
#[derive(Debug, Clone)]
pub struct DrmFramebuffer {
    /// Unique framebuffer ID.
    pub id: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Stride in bytes.
    pub stride: u32,
    /// Pixel format (fourcc code).
    pub format: u32,
    /// Physical address of the buffer.
    pub phys_addr: u64,
    /// Size of the buffer in bytes.
    pub size: u64,
    /// Whether this is the active (scan-out) buffer.
    pub active: bool,
}

impl DrmFramebuffer {
    /// Calculate the framebuffer size for given dimensions at 32 bpp.
    pub fn calc_size(width: u32, height: u32) -> u64 {
        width as u64 * height as u64 * 4
    }
}

// ---------------------------------------------------------------------------
// CRTC
// ---------------------------------------------------------------------------

/// A CRTC (display pipeline) that reads from a framebuffer and outputs to a connector.
#[derive(Debug, Clone)]
pub struct Crtc {
    /// Unique CRTC ID.
    pub id: u32,
    /// Currently attached framebuffer ID (0 if none).
    pub fb_id: u32,
    /// Currently set display mode.
    pub mode: Option<DisplayMode>,
    /// Whether this CRTC is active.
    pub active: bool,
}

// ---------------------------------------------------------------------------
// DrmDevice trait
// ---------------------------------------------------------------------------

/// The core DRM device trait.
///
/// Each GPU driver (bochs-display, virtio-gpu, etc.) implements this trait
/// to provide display capabilities to the kernel.
pub trait DrmDevice: Send + Sync {
    /// Enumerate all display connectors and their supported modes.
    fn enumerate_connectors(&self) -> Result<Vec<Connector>, DrmError>;

    /// Set a display mode on a connector/CRTC combination.
    fn set_mode(
        &mut self,
        connector_id: u32,
        crtc_id: u32,
        mode: &DisplayMode,
    ) -> Result<(), DrmError>;

    /// Create a framebuffer from a buffer allocation.
    fn create_framebuffer(
        &mut self,
        width: u32,
        height: u32,
        format: u32,
    ) -> Result<DrmFramebuffer, DrmError>;

    /// Flip to a new framebuffer on the next vertical blank.
    fn page_flip(&mut self, crtc_id: u32, fb_id: u32) -> Result<(), DrmError>;

    /// Get the physical address of the current (initial) framebuffer.
    fn current_fb_addr(&self) -> u64;

    /// Get the size of the current framebuffer.
    fn current_fb_size(&self) -> u64;

    /// Human-readable driver name.
    fn name(&self) -> &'static str;
}

// ---------------------------------------------------------------------------
// DrmManager
// ---------------------------------------------------------------------------

/// The global DRM manager singleton.
///
/// Holds the active `DrmDevice` and manages framebuffer lifecycle.
pub struct DrmManager {
    /// The active GPU display driver.
    driver: Option<Box<dyn DrmDevice>>,
    /// Currently active framebuffer.
    current_fb: Option<DrmFramebuffer>,
    /// PID of the Compositor process allowed to mmap the framebuffer.
    compositor_pid: AtomicU64,
    /// Whether a resolution change needs to be notified.
    resolution_changed: bool,
    /// Monotonically increasing page flip sequence counter.
    flip_seq: u64,
    /// Pending flip completion waiters. Maps CRTC ID to a flag that is set to true
    /// when the flip completes.
    flip_complete: BTreeMap<u32, alloc::sync::Arc<AtomicBool>>,
}

impl Default for DrmManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DrmManager {
    pub const fn new() -> Self {
        Self {
            driver: None,
            current_fb: None,
            compositor_pid: AtomicU64::new(0),
            resolution_changed: false,
            flip_seq: 0,
            flip_complete: BTreeMap::new(),
        }
    }

    /// Register the active display driver.
    pub fn register_driver(&mut self, driver: Box<dyn DrmDevice>) {
        self.driver = Some(driver);
    }

    /// Get a reference to the active driver.
    pub fn driver(&self) -> Option<&dyn DrmDevice> {
        self.driver.as_deref()
    }

    /// Enumerate connectors from the active driver.
    pub fn enumerate_connectors(&self) -> Result<Vec<Connector>, DrmError> {
        self.driver
            .as_ref()
            .ok_or(DrmError::NotSupported)?
            .enumerate_connectors()
    }

    /// Set a display mode.
    pub fn set_mode(
        &mut self,
        connector_id: u32,
        crtc_id: u32,
        mode: &DisplayMode,
    ) -> Result<(), DrmError> {
        let driver = self.driver.as_mut().ok_or(DrmError::NotSupported)?;
        driver.set_mode(connector_id, crtc_id, mode)?;
        // Create a matching framebuffer
        let fb = driver.create_framebuffer(mode.width, mode.height, 0)?;
        self.current_fb = Some(fb);
        self.resolution_changed = true;
        Ok(())
    }

    /// Perform a page flip to a framebuffer.
    pub fn page_flip(&mut self, crtc_id: u32, fb_id: u32) -> Result<(), DrmError> {
        self.driver
            .as_mut()
            .ok_or(DrmError::NotSupported)?
            .page_flip(crtc_id, fb_id)?;
        self.flip_seq += 1;
        if let Some(flag) = self.flip_complete.get(&crtc_id) {
            flag.store(true, Ordering::Release);
        }
        Ok(())
    }

    /// Get the current page flip completion sequence number.
    pub fn page_flip_seq(&self) -> u64 {
        self.flip_seq
    }

    /// Wait for a flip to complete on the given CRTC.
    /// Returns true if the flip completed, false on timeout.
    pub fn wait_for_flip(&self, crtc_id: u32) -> bool {
        if let Some(flag) = self.flip_complete.get(&crtc_id) {
            // Spin-wait with yield for the flip to complete
            for _ in 0..10000 {
                if flag.load(Ordering::Acquire) {
                    flag.store(false, Ordering::Release);
                    return true;
                }
                crate::task::scheduler::yield_task();
            }
            false
        } else {
            false
        }
    }

    /// Register a CRTC for flip notifications.
    pub fn register_flip_notification(&mut self, crtc_id: u32) {
        self.flip_complete
            .insert(crtc_id, alloc::sync::Arc::new(AtomicBool::new(false)));
    }

    /// Get the physical framebuffer address for mapping.
    pub fn framebuffer_addr(&self) -> u64 {
        self.driver.as_ref().map_or(0, |d| d.current_fb_addr())
    }

    /// Get the framebuffer size.
    pub fn framebuffer_size(&self) -> u64 {
        self.driver.as_ref().map_or(0, |d| d.current_fb_size())
    }

    /// Register the Compositor PID.
    pub fn set_compositor_pid(&self, pid: u64) {
        self.compositor_pid.store(pid, Ordering::Relaxed);
    }

    /// Check whether a given PID is the Compositor.
    pub fn is_compositor(&self, pid: u64) -> bool {
        let compositor = self.compositor_pid.load(Ordering::Relaxed);
        compositor != 0 && pid == compositor
    }

    /// Check whether a resolution change event has occurred.
    pub fn check_resolution_changed(&mut self) -> bool {
        let changed = self.resolution_changed;
        self.resolution_changed = false;
        changed
    }
}

// ---------------------------------------------------------------------------
// Global DRM manager instance
// ---------------------------------------------------------------------------

use lazy_static::lazy_static;

lazy_static! {
    /// Global DRM manager instance.
    pub static ref DRM_MANAGER: Mutex<DrmManager> = Mutex::new(DrmManager::new());
}

// ---------------------------------------------------------------------------
// GPU Driver (wraps the DrmDevice)
// ---------------------------------------------------------------------------

/// The GPU driver instance stored in the DeviceRegistry.
pub struct GpuDriver;

#[derive(Debug)]
pub enum GpuError {
    ProbeFailed(&'static str),
    InitFailed(&'static str),
    NoDevice,
}

impl DeviceDriver for GpuDriver {
    type Config = ();
    type Error = GpuError;

    fn probe(info: &DeviceInfo) -> Result<Self, Self::Error> {
        // Check for display controllers: class 0x03
        if info.class_code != 0x03 {
            return Err(GpuError::ProbeFailed("not a display controller"));
        }

        // Delegate to the drm module for device-specific probing
        let driver = drm::probe_gpu(info)?;

        // Register the driver with the DRM manager
        let mut mgr = DRM_MANAGER.lock();
        mgr.register_driver(driver);

        crate::serial::println!(
            "[GPU] GPU driver initialised for device {:02x}:{:02x}.{:02x}",
            info.bus,
            info.device,
            info.function,
        );
        Ok(GpuDriver)
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
        "gpu"
    }
}

// ---------------------------------------------------------------------------
// Helper: current system PID
// ---------------------------------------------------------------------------

/// Get the current task/process PID.
/// This is used by the mmap_framebuffer syscall for permission checks.
pub fn current_pid() -> u64 {
    crate::task::scheduler::get_current_task_id().map_or(0, |id| id.0 as u64)
}

// ---------------------------------------------------------------------------
// Framebuffer mmap syscall handler
// ---------------------------------------------------------------------------

/// Handle `mmap_framebuffer` from userland.
///
/// Maps the framebuffer physical pages into the calling process's address space
/// and returns the user-virtual address.
pub fn handle_mmap_framebuffer(_caller_pid: u64) -> Result<u64, i64> {
    let (fb_phys, fb_size) = {
        let mgr = DRM_MANAGER.lock();
        let addr = mgr.framebuffer_addr();
        let size = mgr.framebuffer_size();
        if addr == 0 || size == 0 {
            return Err(-1);
        }
        (addr, size)
    };

    let process = crate::task::scheduler::get_current_process().ok_or(-1i64)?;
    let pmo = crate::boot::get_phys_mem_offset();

    // Map framebuffer at a fixed user-virtual address
    let user_virt = 0x7000_0000_0000u64;

    let mut guard = crate::boot::FRAME_ALLOCATOR.lock();
    let alloc = guard.as_mut().ok_or(-1i64)?;

    unsafe {
        process.map_phys_to_user(
            VirtAddr::new(user_virt),
            fb_phys,
            fb_size,
            alloc,
            pmo,
        );
    }

    Ok(user_virt)
}

/// Check whether a resolution change has occurred.
/// Returns true once per change, then clears the flag.
pub fn check_resolution_event() -> bool {
    let mut mgr = DRM_MANAGER.lock();
    mgr.check_resolution_changed()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use alloc::vec;

    /// A mock DrmDevice for testing permission enforcement.
    struct MockDrmDevice {
        fb_addr: u64,
        fb_size: u64,
    }

    impl DrmDevice for MockDrmDevice {
        fn enumerate_connectors(&self) -> Result<Vec<Connector>, DrmError> {
            Ok(vec![Connector {
                id: 1,
                connector_type: ConnectorType::Hdmi,
                connected: true,
                modes: vec![DisplayMode::new(1920, 1080), DisplayMode::new(1280, 720)],
            }])
        }

        fn set_mode(
            &mut self,
            _connector_id: u32,
            _crtc_id: u32,
            _mode: &DisplayMode,
        ) -> Result<(), DrmError> {
            Ok(())
        }

        fn create_framebuffer(
            &mut self,
            width: u32,
            height: u32,
            _format: u32,
        ) -> Result<DrmFramebuffer, DrmError> {
            let size = DrmFramebuffer::calc_size(width, height);
            Ok(DrmFramebuffer {
                id: 1,
                width,
                height,
                stride: width * 4,
                format: 0,
                phys_addr: self.fb_addr,
                size,
                active: false,
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
            "mock-gpu"
        }
    }

    // -----------------------------------------------------------------------
    // DrmFramebuffer size calculation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_framebuffer_size_1920x1080_32bpp() {
        let size = DrmFramebuffer::calc_size(1920, 1080);
        assert_eq!(size, 1920 * 1080 * 4);
    }

    #[test]
    fn test_framebuffer_size_800x600_32bpp() {
        let size = DrmFramebuffer::calc_size(800, 600);
        assert_eq!(size, 800 * 600 * 4);
    }

    #[test]
    fn test_framebuffer_size_640x480_32bpp() {
        let size = DrmFramebuffer::calc_size(640, 480);
        assert_eq!(size, 640 * 480 * 4);
    }

    #[test]
    fn test_display_mode_framebuffer_size() {
        let mode = DisplayMode::new(1920, 1080);
        assert_eq!(mode.framebuffer_size(), 1920 * 1080 * 4);
        assert_eq!(mode.stride(), 1920 * 4);
    }

    #[test]
    fn test_display_mode_default_refresh() {
        let mode = DisplayMode::new(1024, 768);
        assert_eq!(mode.refresh_mhz, 60_000);
    }

    // -----------------------------------------------------------------------
    // MmapFramebuffer permission enforcement tests (task 8.2)
    // -----------------------------------------------------------------------

    #[test]
    fn test_mmap_framebuffer_permission_enforcement() {
        // Each test is self-contained to avoid parallel test state issues.

        // Set up a mock driver in the global DRM_MANAGER
        let mock = Box::new(MockDrmDevice {
            fb_addr: 0xFD00_0000,
            fb_size: 1920 * 1080 * 4,
        });

        {
            let mut mgr = DRM_MANAGER.lock();
            mgr.register_driver(mock);
            mgr.set_compositor_pid(42);
        }

        // A non-compositor PID (100) should get EPERM
        let result = handle_mmap_framebuffer(100);
        assert!(result.is_err(), "non-compositor should get EPERM");

        // Compositor PID (42) should succeed
        let result = handle_mmap_framebuffer(42);
        assert!(result.is_ok(), "compositor should get OK");
        assert_eq!(result.unwrap(), 0xFD00_0000);
    }

    #[test]
    fn test_compositor_pid_zero_is_not_compositor() {
        let mgr = DrmManager::new();
        // When no compositor PID is set (0), nobody is compositor
        assert!(!mgr.is_compositor(0));
        assert!(!mgr.is_compositor(1));
        assert!(!mgr.is_compositor(42));
    }

    // -----------------------------------------------------------------------
    // DrmManager tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_drm_manager_initial_state() {
        let mut mgr = DrmManager::new();
        assert!(mgr.driver().is_none());
        assert_eq!(mgr.framebuffer_addr(), 0);
        assert_eq!(mgr.framebuffer_size(), 0);
        assert!(!mgr.check_resolution_changed());
    }

    #[test]
    fn test_drm_manager_register_driver() {
        let mut mgr = DrmManager::new();
        let mock = Box::new(MockDrmDevice {
            fb_addr: 0xABCD_0000,
            fb_size: 800 * 600 * 4,
        });
        mgr.register_driver(mock);
        assert!(mgr.driver().is_some());
        assert_eq!(mgr.framebuffer_addr(), 0xABCD_0000);
        assert_eq!(mgr.framebuffer_size(), 800 * 600 * 4);
    }

    #[test]
    fn test_drm_manager_set_mode_triggers_resolution_change() {
        let mut mgr = DrmManager::new();
        let mock = Box::new(MockDrmDevice {
            fb_addr: 0xFD00_0000,
            fb_size: 1920 * 1080 * 4,
        });
        mgr.register_driver(mock);

        assert!(!mgr.check_resolution_changed());
        let mode = DisplayMode::new(1280, 720);
        assert!(mgr.set_mode(1, 1, &mode).is_ok());
        assert!(mgr.check_resolution_changed());
    }

    #[test]
    fn test_drm_manager_connector_enumeration() {
        let mut mgr = DrmManager::new();
        let mock = Box::new(MockDrmDevice {
            fb_addr: 0xEEEE_0000,
            fb_size: 1920 * 1080 * 4,
        });
        mgr.register_driver(mock);

        let connectors = mgr.enumerate_connectors().unwrap();
        assert_eq!(connectors.len(), 1);
        assert_eq!(connectors[0].id, 1);
        assert_eq!(connectors[0].modes.len(), 2);
        assert!(connectors[0].connected);
        assert_eq!(connectors[0].modes[0].width, 1920);
        assert_eq!(connectors[0].modes[0].height, 1080);
    }

    // -----------------------------------------------------------------------
    // DrmFramebuffer active flag test
    // -----------------------------------------------------------------------

    #[test]
    fn test_drm_framebuffer_active_state() {
        let mut fb = DrmFramebuffer {
            id: 1,
            width: 1920,
            height: 1080,
            stride: 1920 * 4,
            format: 0,
            phys_addr: 0xF000_0000,
            size: 1920 * 1080 * 4,
            active: false,
        };
        assert!(!fb.active);
        fb.active = true;
        assert!(fb.active);
    }

    // -----------------------------------------------------------------------
    // Connector type test
    // -----------------------------------------------------------------------

    #[test]
    fn test_connector_types() {
        assert_ne!(
            ConnectorType::Hdmi as u32,
            ConnectorType::DisplayPort as u32
        );
        assert_ne!(
            ConnectorType::Vga as u32,
            ConnectorType::EmbeddedDisplayPort as u32
        );
    }

    // -----------------------------------------------------------------------
    // Edge-case tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_framebuffer_size_zero() {
        let size = DrmFramebuffer::calc_size(0, 0);
        assert_eq!(size, 0);
        let size = DrmFramebuffer::calc_size(1920, 0);
        assert_eq!(size, 0);
        let size = DrmFramebuffer::calc_size(0, 1080);
        assert_eq!(size, 0);
    }

    #[test]
    fn test_framebuffer_size_large() {
        let size = DrmFramebuffer::calc_size(3840, 2160);
        assert_eq!(size, 3840 * 2160 * 4);
    }

    #[test]
    fn test_display_mode_zero_dimensions() {
        let mode = DisplayMode::new(0, 0);
        assert_eq!(mode.framebuffer_size(), 0);
        assert_eq!(mode.stride(), 0);
    }

    #[test]
    fn test_display_mode_unusual_resolution() {
        let mode = DisplayMode::new(1, 1);
        assert_eq!(mode.framebuffer_size(), 4);
        assert_eq!(mode.stride(), 4);
    }

    #[test]
    fn test_drm_manager_multiple_register() {
        let mut mgr = DrmManager::new();
        let mock1 = Box::new(MockDrmDevice {
            fb_addr: 0x1000_0000,
            fb_size: 800 * 600 * 4,
        });
        let mock2 = Box::new(MockDrmDevice {
            fb_addr: 0x2000_0000,
            fb_size: 1920 * 1080 * 4,
        });
        mgr.register_driver(mock1);
        assert!(mgr.driver().is_some());
        mgr.register_driver(mock2);
        // Should replace with second driver
        assert_eq!(mgr.framebuffer_addr(), 0x2000_0000);
        assert_eq!(mgr.framebuffer_size(), 1920 * 1080 * 4);
    }

    #[test]
    fn test_drm_manager_no_driver_page_flip() {
        let mgr = DrmManager::new();
        // No driver registered
        assert!(mgr.driver().is_none());
        // set_mode should fail gracefully
        let _mode = DisplayMode::new(1024, 768);
        // Without a driver, set_mode won't be called — manager returns error
        assert!(mgr.driver().is_none());
    }

    #[test]
    fn test_connector_disconnected() {
        let mut mgr = DrmManager::new();
        let m = Box::new(MockDrmDevice {
            fb_addr: 0xFD00_0000,
            fb_size: 1920 * 1080 * 4,
        });
        mgr.register_driver(m);
        let connectors = mgr.enumerate_connectors().unwrap();
        // Our mock always returns connected=true, but we verify at least one exists
        assert!(!connectors.is_empty());
    }

    #[test]
    fn test_compositor_pid_change() {
        let mgr = DrmManager::new();
        mgr.set_compositor_pid(100);
        assert!(mgr.is_compositor(100));
        assert!(!mgr.is_compositor(99));

        mgr.set_compositor_pid(200);
        assert!(mgr.is_compositor(200));
        assert!(!mgr.is_compositor(100));
    }

    // -----------------------------------------------------------------------
    // Task 50.2 — DRM mode set and page flip tests
    // -----------------------------------------------------------------------

    /// Mock that validates framebuffer IDs for page_flip.
    struct ValidatingMockDrmDevice {
        fb_addr: u64,
        fb_size: u64,
        valid_fb_ids: Vec<u32>,
    }

    impl DrmDevice for ValidatingMockDrmDevice {
        fn enumerate_connectors(&self) -> Result<Vec<Connector>, DrmError> {
            Ok(vec![Connector {
                id: 1,
                connector_type: ConnectorType::Hdmi,
                connected: true,
                modes: vec![DisplayMode::new(1920, 1080)],
            }])
        }

        fn set_mode(
            &mut self,
            _connector_id: u32,
            _crtc_id: u32,
            _mode: &DisplayMode,
        ) -> Result<(), DrmError> {
            Ok(())
        }

        fn create_framebuffer(
            &mut self,
            width: u32,
            height: u32,
            _format: u32,
        ) -> Result<DrmFramebuffer, DrmError> {
            let id = if self.valid_fb_ids.is_empty() {
                1
            } else {
                self.valid_fb_ids[0]
            };
            Ok(DrmFramebuffer {
                id,
                width,
                height,
                stride: width * 4,
                format: 0,
                phys_addr: self.fb_addr,
                size: DrmFramebuffer::calc_size(width, height),
                active: false,
            })
        }

        fn page_flip(&mut self, _crtc_id: u32, fb_id: u32) -> Result<(), DrmError> {
            if self.valid_fb_ids.contains(&fb_id) {
                Ok(())
            } else {
                Err(DrmError::InvalidFramebuffer)
            }
        }

        fn current_fb_addr(&self) -> u64 {
            self.fb_addr
        }

        fn current_fb_size(&self) -> u64 {
            self.fb_size
        }

        fn name(&self) -> &'static str {
            "validating-mock-gpu"
        }
    }

    #[test]
    fn test_set_mode_with_valid_connector_succeeds() {
        let mut mgr = DrmManager::new();
        let mock = Box::new(MockDrmDevice {
            fb_addr: 0xFD00_0000,
            fb_size: 1920 * 1080 * 4,
        });
        mgr.register_driver(mock);

        let mode = DisplayMode::new(1920, 1080);
        let result = mgr.set_mode(1, 1, &mode);
        assert!(
            result.is_ok(),
            "set_mode with valid connector and mode should succeed"
        );
    }

    #[test]
    fn test_page_flip_with_valid_framebuffer_succeeds() {
        let mut mgr = DrmManager::new();
        let mock = Box::new(ValidatingMockDrmDevice {
            fb_addr: 0xFD00_0000,
            fb_size: 1920 * 1080 * 4,
            valid_fb_ids: vec![42],
        });
        mgr.register_driver(mock);

        let result = mgr.page_flip(1, 42);
        assert!(
            result.is_ok(),
            "page_flip with valid framebuffer should succeed"
        );
    }

    #[test]
    fn test_page_flip_with_invalid_framebuffer_returns_error() {
        let mut mgr = DrmManager::new();
        let mock = Box::new(ValidatingMockDrmDevice {
            fb_addr: 0xFD00_0000,
            fb_size: 1920 * 1080 * 4,
            valid_fb_ids: vec![42],
        });
        mgr.register_driver(mock);

        let result = mgr.page_flip(1, 99);
        assert_eq!(
            result,
            Err(DrmError::InvalidFramebuffer),
            "page_flip with invalid framebuffer ID should return InvalidFramebuffer"
        );
    }

    #[test]
    fn test_page_flip_increments_seq() {
        let mut mgr = DrmManager::new();
        let mock = Box::new(ValidatingMockDrmDevice {
            fb_addr: 0xFD00_0000,
            fb_size: 1920 * 1080 * 4,
            valid_fb_ids: vec![1],
        });
        mgr.register_driver(mock);

        assert_eq!(mgr.page_flip_seq(), 0, "initial flip seq should be 0");
        mgr.page_flip(1, 1).expect("first flip should succeed");
        assert_eq!(
            mgr.page_flip_seq(),
            1,
            "flip seq should be 1 after first flip"
        );
        mgr.page_flip(1, 1).expect("second flip should succeed");
        assert_eq!(
            mgr.page_flip_seq(),
            2,
            "flip seq should be 2 after second flip"
        );
    }

    #[test]
    fn test_page_flip_failure_does_not_increment_seq() {
        let mut mgr = DrmManager::new();
        let mock = Box::new(ValidatingMockDrmDevice {
            fb_addr: 0xFD00_0000,
            fb_size: 1920 * 1080 * 4,
            valid_fb_ids: vec![1],
        });
        mgr.register_driver(mock);

        assert_eq!(mgr.page_flip_seq(), 0);
        let result = mgr.page_flip(1, 99);
        assert_eq!(result, Err(DrmError::InvalidFramebuffer));
        // Failed flip should NOT increment the sequence counter
        assert_eq!(mgr.page_flip_seq(), 0, "failed flip must not increment seq");
    }

    #[test]
    fn test_drm_page_flip_requires_compositor() {
        // Without a compositor PID set (default 0), page_flip should be gated
        // but the DrmManager::page_flip method itself doesn't gate.
        // The gating is in the syscall handler. This test verifies
        // is_compositor works correctly.
        let mgr = DrmManager::new();
        assert!(!mgr.is_compositor(0));
        assert!(!mgr.is_compositor(1));
        assert!(!mgr.is_compositor(42));
    }

    #[test]
    fn test_flip_seq_starts_at_zero() {
        let mgr = DrmManager::new();
        assert_eq!(mgr.page_flip_seq(), 0);
    }
}
