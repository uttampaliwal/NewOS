pub mod psf;

use lazy_static::lazy_static;
use turnix_abi::boot::BootFramebuffer;
use spin::Mutex;
use psf::Psf2Font;

lazy_static! {
    pub static ref FRAMEBUFFER: Mutex<Option<Framebuffer>> = Mutex::new(None);
    pub static ref CONSOLE: Mutex<Option<TextConsole<'static>>> = Mutex::new(None);
    pub static ref FONT_DATA: Mutex<Option<alloc::vec::Vec<u8>>> = Mutex::new(None);
}

pub struct Framebuffer {
    addr: u64,
    width: u32,
    height: u32,
    pitch: u32,
    format: u32,
}

pub struct TextConsole<'a> {
    font: Psf2Font<'a>,
    cursor_x: u32,
    cursor_y: u32,
    foreground: u32,
    background: u32,
}

impl<'a> TextConsole<'a> {
    pub fn new(font: Psf2Font<'a>) -> Self {
        Self {
            font,
            cursor_x: 0,
            cursor_y: 0,
            foreground: 0xFFFFFF, // White
            background: 0x001a2a, // Dark blue
        }
    }

    pub fn write_char(&mut self, c: char) {
        if c == '\n' {
            self.newline();
            return;
        }

        if let Some(glyph) = self.font.get_glyph(c) {
            let mut fb_lock = FRAMEBUFFER.lock();
            if let Some(ref mut fb) = *fb_lock {
                let font_width = self.font.header.width;
                let font_height = self.font.header.height;
                let bytes_per_line = (font_width + 7) / 8;

                if self.cursor_x + font_width > fb.width {
                    self.newline_locked(fb);
                }

                for row in 0..font_height {
                    for col in 0..font_width {
                        let byte_idx = (row * bytes_per_line + (col / 8)) as usize;
                        let bit_idx = 7 - (col % 8);
                        let is_set = (glyph[byte_idx] >> bit_idx) & 1 == 1;

                        let color = if is_set { self.foreground } else { self.background };
                        fb.set_pixel(self.cursor_x + col, self.cursor_y + row, color);
                    }
                }
                self.cursor_x += font_width;
            }
        }
    }

    pub fn backspace(&mut self) {
        let font_width = self.font.header.width;
        let font_height = self.font.header.height;

        if self.cursor_x >= font_width {
            self.cursor_x -= font_width;
            let mut fb_lock = FRAMEBUFFER.lock();
            if let Some(ref mut fb) = *fb_lock {
                fb.draw_rect(self.cursor_x, self.cursor_y, font_width, font_height, self.background);
            }
        }
    }

    fn newline(&mut self) {
        let mut fb_lock = FRAMEBUFFER.lock();
        if let Some(ref mut fb) = *fb_lock {
            self.newline_locked(fb);
        }
    }

    fn newline_locked(&mut self, fb: &mut Framebuffer) {
        self.cursor_x = 0;
        let font_height = self.font.header.height;
        if self.cursor_y + 2 * font_height > fb.height {
            self.scroll(fb);
        } else {
            self.cursor_y += font_height;
        }
    }

    fn scroll(&mut self, fb: &mut Framebuffer) {
        let font_height = self.font.header.height;
        let line_size = (fb.pitch * font_height) as usize * 4;
        let total_size = (fb.pitch * fb.height) as usize * 4;

        unsafe {
            let dest = fb.addr as *mut u8;
            let src = (fb.addr + line_size as u64) as *const u8;
            core::ptr::copy(src, dest, total_size - line_size);

            // Clear last line
            let last_line_ptr = (fb.addr + (total_size - line_size) as u64) as *mut u32;
            for i in 0..(line_size / 4) {
                last_line_ptr.add(i).write_volatile(self.background);
            }
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for c in s.chars() {
            self.write_char(c);
        }
    }
}

pub fn init_console(data: alloc::vec::Vec<u8>) {
    let leaked_data = alloc::boxed::Box::leak(data.into_boxed_slice());
    if let Some(font) = Psf2Font::new(leaked_data) {
        let mut console = CONSOLE.lock();
        *console = Some(TextConsole::new(font));
    }
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
        // Clear screen with a nice dark blue for turnix
        f.clear(0x001a2a);

        // Draw a small "logo" placeholder
        f.draw_rect(20, 20, 100, 100, 0x00aaff);
        f.draw_rect(140, 20, 100, 100, 0xffaa00);
        f.draw_rect(260, 20, 100, 100, 0x00ffaa);
    }
}
