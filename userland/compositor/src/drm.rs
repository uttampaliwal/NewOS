/// Display backend abstraction over the Turnix DRM/GBM subsystem.
pub struct DrmBackend {
    /// Physical address of the scanout framebuffer.
    pub fb_addr: u64,
    /// Size of the framebuffer in bytes.
    pub fb_size: u64,
    /// Display width in pixels.
    pub width: u32,
    /// Display height in pixels.
    pub height: u32,
    /// GBM buffer ID for the back buffer (0 = none).
    pub back_buffer: u64,
}

impl DrmBackend {
    /// Initialise the display backend by querying the framebuffer.
    pub fn init() -> Option<Self> {
        let fb_addr = libturnix::mmap_framebuffer()?;
        let width = 1920;
        let height = 1080;
        let fb_size = (width as u64) * (height as u64) * 4;
        Some(Self {
            fb_addr,
            fb_size,
            width,
            height,
            back_buffer: 0,
        })
    }

    /// Enable the display (no-op for bochs, which is always on).
    pub fn enable(&mut self) {}
}
