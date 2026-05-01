#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Psf2Header {
    pub magic: [u8; 4],
    pub version: u32,
    pub header_size: u32,
    pub flags: u32,
    pub length: u32,
    pub char_size: u32,
    pub height: u32,
    pub width: u32,
}

pub struct Psf2Font<'a> {
    pub header: &'a Psf2Header,
    pub glyphs: &'a [u8],
}

impl<'a> Psf2Font<'a> {
    pub fn new(data: &'a [u8]) -> Option<Self> {
        if data.len() < core::mem::size_of::<Psf2Header>() {
            return None;
        }

        let header = unsafe { &*(data.as_ptr() as *const Psf2Header) };

        // Magic number for PSF2: 0x86 0x4a 0xb5 0x72
        if header.magic != [0x72, 0xb5, 0x4a, 0x86] {
            return None;
        }

        let glyphs_start = header.header_size as usize;
        let glyphs_end = glyphs_start + (header.length * header.char_size) as usize;

        if data.len() < glyphs_end {
            return None;
        }

        Some(Self {
            header,
            glyphs: &data[glyphs_start..glyphs_end],
        })
    }

    pub fn get_glyph(&self, c: char) -> Option<&[u8]> {
        let index = c as usize;
        if index >= self.header.length as usize {
            return None;
        }

        let start = index * self.header.char_size as usize;
        let end = start + self.header.char_size as usize;

        Some(&self.glyphs[start..end])
    }
}
