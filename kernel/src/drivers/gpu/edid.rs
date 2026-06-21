//! EDID (Extended Display Identification Data) parser.
//!
//! Parses EDID 1.3 data structures to extract display capabilities
//! and preferred modes. Reference: VESA EDID standard.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// EDID block size (128 bytes per block).
pub const EDID_BLOCK_SIZE: usize = 128;

/// EDID magic header.
pub const EDID_MAGIC: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

/// A parsed EDID display mode.
#[derive(Debug, Clone, Copy)]
pub struct EdidMode {
    /// Horizontal resolution in pixels.
    pub h_pixels: u16,
    /// Vertical resolution in lines.
    pub v_lines: u16,
    /// Refresh rate in Hz (e.g., 60, 75, 144).
    pub refresh_hz: u16,
    /// Pixel clock in kHz.
    pub pixel_clock_khz: u32,
    /// Horizontal blanking pixels.
    pub h_blank: u16,
    /// Vertical blanking lines.
    pub v_blank: u16,
    /// Is this the preferred mode?
    pub preferred: bool,
}

/// Parsed EDID data.
#[derive(Debug, Clone)]
pub struct EdidData {
    /// Manufacturer ID (3 letters, e.g., "DEL", "SAM").
    pub manufacturer: [char; 3],
    /// Product code.
    pub product_code: u16,
    /// Serial number.
    pub serial: u32,
    /// Week of manufacture.
    pub week: u8,
    /// Year of manufacture (offset from 1990).
    pub year: u8,
    /// EDID version.
    pub edid_version: u8,
    /// EDID revision.
    pub edid_revision: u8,
    /// Maximum horizontal image size in cm.
    pub max_h_size_cm: u8,
    /// Maximum vertical image size in cm.
    pub max_v_size_cm: u8,
    /// Gamma value (gamma * 100, e.g., 220 = 2.2).
    pub gamma: u8,
    /// Supported features bitmask.
    pub features: u8,
    /// Available display modes.
    pub modes: Vec<EdidMode>,
    /// Monitor name (from descriptor blocks).
    pub monitor_name: String,
    /// Preferred mode (first from EDID timing or descriptor).
    pub preferred_mode: Option<EdidMode>,
}

impl EdidData {
    /// Parse EDID data from raw bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < EDID_BLOCK_SIZE {
            return None;
        }

        // Verify magic header
        if data[0..8] != EDID_MAGIC {
            return None;
        }

        // Manufacturer ID (bytes 8-9, encoded as 5-bit per char)
        let mfg_bytes = u16::from_le_bytes([data[8], data[9]]);
        let manufacturer = [
            b'A' + ((mfg_bytes >> 10) & 0x1F) as u8,
            b'A' + ((mfg_bytes >> 5) & 0x1F) as u8,
            b'A' + (mfg_bytes & 0x1F) as u8,
        ];

        let product_code = u16::from_le_bytes([data[10], data[11]]);
        let serial = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        let week = data[16];
        let year = data[17]; // Year of manufacture = 1990 + value
        let edid_version = data[18];
        let edid_revision = data[19];

        // Basic display parameters
        let max_h_size_cm = data[21];
        let max_v_size_cm = data[22];
        let gamma = data[23];
        let features = data[24];

        // Parse timing descriptors (54-125, 4 descriptors × 18 bytes each)
        let mut modes = Vec::new();

        for i in 0..4 {
            let offset = 54 + i * 18;
            if offset + 18 > data.len() {
                break;
            }

            let desc = &data[offset..offset + 18];

            // Check if this is a timing descriptor (first 2 bytes are non-zero)
            if desc[0] != 0 || desc[1] != 0 {
                // Standard timing descriptor — parse pixel clock and dimensions
                let pixel_clock = u16::from_le_bytes([desc[0], desc[1]]);
                if pixel_clock == 0 {
                    continue;
                }

                let h_pixels = (desc[2] as u16 + 31) * 8;
                let aspect = (desc[3] >> 6) & 0x03;
                let v_lines = match aspect {
                    0 => (h_pixels * 10) / 16, // 16:10
                    1 => (h_pixels * 3) / 4,   // 4:3
                    2 => (h_pixels * 9) / 16,  // 16:9
                    3 => h_pixels,              // 5:4
                    _ => (h_pixels * 3) / 4,
                };

                let refresh_hz = 60 + ((desc[3] & 0x3F) as u16);

                let mode = EdidMode {
                    h_pixels,
                    v_lines,
                    refresh_hz,
                    pixel_clock_khz: pixel_clock as u32 * 10,
                    h_blank: 0,
                    v_blank: 0,
                    preferred: false,
                };
                modes.push(mode);
            }
        }

        // Add common VESA modes if none parsed
        if modes.is_empty() {
            modes.push(EdidMode {
                h_pixels: 640,
                v_lines: 480,
                refresh_hz: 60,
                pixel_clock_khz: 25175,
                h_blank: 160,
                v_blank: 45,
                preferred: false,
            });
            modes.push(EdidMode {
                h_pixels: 800,
                v_lines: 600,
                refresh_hz: 60,
                pixel_clock_khz: 40000,
                h_blank: 160,
                v_blank: 28,
                preferred: false,
            });
            modes.push(EdidMode {
                h_pixels: 1024,
                v_lines: 768,
                refresh_hz: 60,
                pixel_clock_khz: 65000,
                h_blank: 160,
                v_blank: 29,
                preferred: false,
            });
            modes.push(EdidMode {
                h_pixels: 1280,
                v_lines: 720,
                refresh_hz: 60,
                pixel_clock_khz: 74250,
                h_blank: 370,
                v_blank: 30,
                preferred: false,
            });
            modes.push(EdidMode {
                h_pixels: 1920,
                v_lines: 1080,
                refresh_hz: 60,
                pixel_clock_khz: 148500,
                h_blank: 280,
                v_blank: 45,
                preferred: true,
            });
        }

        // Extract monitor name from descriptor blocks
        let mut monitor_name = String::from("Unknown");
        // Check descriptor blocks for name tag (0xFC = monitor name)
        for i in 0..4 {
            let offset = 54 + i * 18;
            if offset + 18 > data.len() {
                break;
            }
            let desc = &data[offset..offset + 18];
            if desc[0] == 0 && desc[1] == 0 && desc[3] == 0xFC {
                // Monitor name descriptor
                let name_bytes = &desc[5..18];
                let name: String = name_bytes
                    .iter()
                    .take_while(|&&b| b != 0x0A && b != 0)
                    .map(|&b| b as char)
                    .collect();
                if !name.is_empty() {
                    monitor_name = name;
                }
            }
        }

        // Find preferred mode
        let preferred = modes.iter().find(|m| m.preferred).copied();

        Some(Self {
            manufacturer: [
                manufacturer[0] as char,
                manufacturer[1] as char,
                manufacturer[2] as char,
            ],
            product_code,
            serial,
            week,
            year,
            edid_version,
            edid_revision,
            max_h_size_cm,
            max_v_size_cm,
            gamma,
            features,
            modes,
            monitor_name,
            preferred_mode: preferred,
        })
    }

    /// Get the best mode for a given resolution.
    pub fn best_mode(&self, target_w: u16, target_h: u16) -> Option<&EdidMode> {
        self.modes
            .iter()
            .filter(|m| m.h_pixels == target_w && m.v_lines == target_h)
            .max_by_key(|m| m.refresh_hz)
    }

    /// Get all supported resolutions.
    pub fn supported_resolutions(&self) -> Vec<(u16, u16)> {
        let mut res: Vec<(u16, u16)> = self.modes.iter().map(|m| (m.h_pixels, m.v_lines)).collect();
        res.sort();
        res.dedup();
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_edid_header() {
        let mut data = [0u8; 128];
        data[0..8].copy_from_slice(&EDID_MAGIC);
        data[8] = 0x22; // Manufacturer
        data[9] = 0x11;

        let edid = EdidData::parse(&data);
        assert!(edid.is_some());
    }

    #[test]
    fn test_reject_bad_magic() {
        let data = [0u8; 128];
        assert!(EdidData::parse(&data).is_none());
    }

    #[test]
    fn test_reject_short_data() {
        assert!(EdidData::parse(&[0u8; 64]).is_none());
    }

    #[test]
    fn test_common_modes_present() {
        let mut data = [0u8; 128];
        data[0..8].copy_from_slice(&EDID_MAGIC);

        let edid = EdidData::parse(&data).unwrap();
        assert!(edid.modes.len() >= 5);

        // Should have 1920x1080
        let mode_1080 = edid.best_mode(1920, 1080);
        assert!(mode_1080.is_some());
        assert_eq!(mode_1080.unwrap().refresh_hz, 60);
    }

    #[test]
    fn test_supported_resolutions() {
        let mut data = [0u8; 128];
        data[0..8].copy_from_slice(&EDID_MAGIC);

        let edid = EdidData::parse(&data).unwrap();
        let resolutions = edid.supported_resolutions();
        assert!(resolutions.contains(&(1920, 1080)));
        assert!(resolutions.contains(&(1280, 720)));
    }

    #[test]
    fn test_monitor_name_from_descriptor() {
        let mut data = [0u8; 128];
        data[0..8].copy_from_slice(&EDID_MAGIC);

        // Set a monitor name descriptor at offset 54
        data[54] = 0;
        data[55] = 0; // Not a timing descriptor
        data[57] = 0xFC; // Monitor name tag
        data[59..72].copy_from_slice(b"DELL U2723QE\x0A");

        let edid = EdidData::parse(&data).unwrap();
        assert_eq!(edid.monitor_name, "DELL U2723QE");
    }
}
