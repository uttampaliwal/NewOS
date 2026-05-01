use lazy_static::lazy_static;
<<<<<<< HEAD
use turnix_abi::boot::BootFramebuffer;
=======
use newos_abi::boot::BootFramebuffer;
>>>>>>> unstable
use spin::Mutex;

lazy_static! {
    pub static ref FRAMEBUFFER: Mutex<Option<Framebuffer>> = Mutex::new(None);
}

pub struct Framebuffer {
    addr: u64,
    width: u32,
    height: u32,
    pitch: u32,
    format: u32,
}

impl Framebuffer {
    pub fn new(fb: &BootFramebuffer) -> Self {
        Self {
            addr: fb.addr,
            width: fb.width,
            height: fb.height,
            pitch: fb.pitch,
            format: fb.format,
        }
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }

        let pixel_offset = (y * self.pitch + x) as u64 * 4;
        let encoded = match self.format {
            1 => color,
            _ => {
                let red = (color & 0x00ff_0000) >> 16;
                let green = color & 0x0000_ff00;
                let blue = (color & 0x0000_00ff) << 16;
                blue | green | red
            }
        };
        unsafe {
            let ptr = (self.addr + pixel_offset) as *mut u32;
            ptr.write_volatile(encoded);
        }
    }

    pub fn clear(&mut self, color: u32) {
        // Optimization: clear using raw pointers for speed
        for i in 0..(self.height * self.pitch) {
            unsafe {
                let ptr = (self.addr + (i as u64 * 4)) as *mut u32;
                ptr.write_volatile(color);
            }
        }
    }

    pub fn draw_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: u32) {
        for i in y..(y + h) {
            for j in x..(x + w) {
                self.set_pixel(j, i, color);
            }
        }
    }
}

pub fn init(fb_info: &BootFramebuffer) {
    if fb_info.addr == 0 {
        return;
    }

    let mut fb = FRAMEBUFFER.lock();
    *fb = Some(Framebuffer::new(fb_info));

    if let Some(ref mut f) = *fb {
<<<<<<< HEAD
        // Clear screen with a nice dark blue for turnix
=======
        // Clear screen with a nice dark blue for NewOS
>>>>>>> unstable
        f.clear(0x001a2a);

        // Draw a small "logo" placeholder
        f.draw_rect(20, 20, 100, 100, 0x00aaff);
        f.draw_rect(140, 20, 100, 100, 0xffaa00);
        f.draw_rect(260, 20, 100, 100, 0x00ffaa);
    }
}
