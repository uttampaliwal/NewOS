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

        // Safety: data is at least sizeof(Psf2Header) bytes and aligned; cast is valid.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    fn make_valid_psf2_header(char_size: u32, length: u32) -> Vec<u8> {
        let header_size = 32u32;
        let glyph_bytes = char_size * length;
        let total = header_size as usize + glyph_bytes as usize;
        let mut data = vec![0u8; total];
        // Magic
        data[0..4].copy_from_slice(&[0x72, 0xb5, 0x4a, 0x86]);
        // Version
        data[4..8].copy_from_slice(&0u32.to_le_bytes());
        // Header size
        data[8..12].copy_from_slice(&header_size.to_le_bytes());
        // Flags
        data[12..16].copy_from_slice(&0u32.to_le_bytes());
        // Length (num glyphs)
        data[16..20].copy_from_slice(&length.to_le_bytes());
        // Char size (bytes per glyph)
        data[20..24].copy_from_slice(&char_size.to_le_bytes());
        // Height
        data[24..28].copy_from_slice(&16u32.to_le_bytes());
        // Width
        data[28..32].copy_from_slice(&8u32.to_le_bytes());
        data
    }

    #[test]
    fn test_psf2_too_short() {
        let data = vec![0u8; 4];
        assert!(Psf2Font::new(&data).is_none());
    }

    #[test]
    fn test_psf2_bad_magic() {
        let mut data = vec![0u8; 32];
        data[0..4].copy_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        assert!(Psf2Font::new(&data).is_none());
    }

    #[test]
    fn test_psf2_valid_font() {
        let data = make_valid_psf2_header(16, 256);
        let font = Psf2Font::new(&data).unwrap();
        assert_eq!(font.header.length, 256);
        assert_eq!(font.header.char_size, 16);
        assert_eq!(font.glyphs.len(), 256 * 16);
    }

    #[test]
    fn test_psf2_get_glyph_in_bounds() {
        let data = make_valid_psf2_header(16, 256);
        let font = Psf2Font::new(&data).unwrap();
        let glyph = font.get_glyph('A');
        assert!(glyph.is_some());
        assert_eq!(glyph.unwrap().len(), 16);
    }

    #[test]
    fn test_psf2_get_glyph_out_of_bounds() {
        let data = make_valid_psf2_header(16, 1); // Only 1 glyph (index 0)
        let font = Psf2Font::new(&data).unwrap();
        assert!(font.get_glyph('\0').is_some()); // index 0 is valid
        assert!(font.get_glyph('\u{1}').is_none()); // index 1 is out of bounds
    }

    #[test]
    fn test_psf2_truncated_glyph_data() {
        let mut data = make_valid_psf2_header(16, 256);
        data.truncate(32 + 100); // Not enough glyph data
        assert!(Psf2Font::new(&data).is_none());
    }

    #[test]
    fn test_psf2_single_glyph() {
        let data = make_valid_psf2_header(8, 1);
        let font = Psf2Font::new(&data).unwrap();
        assert_eq!(font.header.length, 1);
        assert_eq!(font.header.char_size, 8);
        let glyph = font.get_glyph('\0');
        assert!(glyph.is_some());
        assert_eq!(glyph.unwrap().len(), 8);
    }
}
