#![no_std]
extern crate alloc;

use alloc::string::String;

/// Desktop entry (.desktop file) parser for the application launcher.
pub const APPLICATIONS_DIR: &str = "/usr/share/turnix/applications";

#[derive(Debug, Clone, PartialEq)]
pub struct DesktopEntry {
    pub name: String,
    pub exec: String,
    pub icon: String,
    pub categories: String,
    pub no_display: bool,
    pub terminal: bool,
}

/// Parse a single .desktop file content into a DesktopEntry.
/// Returns None if the file is not a valid Type=Application entry.
pub fn parse_desktop_entry(content: &str) -> Option<DesktopEntry> {
    let mut name = String::new();
    let mut exec = String::new();
    let mut icon = String::new();
    let mut categories = String::new();
    let mut no_display = false;
    let mut terminal = false;
    let mut in_desktop_entry = false;
    let mut is_application = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_desktop_entry = trimmed == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim();
            let value = trimmed[eq_pos + 1..].trim();
            match key {
                "Type" if value == "Application" => is_application = true,
                "Name" => name = String::from(value),
                "Exec" => exec = String::from(value),
                "Icon" => icon = String::from(value),
                "Categories" => categories = String::from(value),
                "NoDisplay" => no_display = value == "true",
                "Terminal" => terminal = value == "true",
                _ => {}
            }
        }
    }

    if !is_application || name.is_empty() || exec.is_empty() {
        return None;
    }
    let exec_clean = String::from(exec.split_whitespace().next().unwrap_or(""));
    Some(DesktopEntry {
        name,
        exec: exec_clean,
        icon,
        categories,
        no_display,
        terminal,
    })
}

/// Fill a pixel buffer with a solid color (BGRA format).
#[allow(clippy::too_many_arguments)]
pub fn fill_rect(pixels: &mut [u8], width: u32, height: u32, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
    for row in y..(y + h).min(height) {
        for col in x..(x + w).min(width) {
            let idx = ((row * width + col) * 4) as usize;
            if idx + 3 < pixels.len() {
                pixels[idx] = color[0];     // B
                pixels[idx + 1] = color[1]; // G
                pixels[idx + 2] = color[2]; // R
                pixels[idx + 3] = color[3]; // A
            }
        }
    }
}

/// Draw a gradient background from top color to bottom color.
pub fn fill_gradient(pixels: &mut [u8], width: u32, height: u32, top: [u8; 4], bottom: [u8; 4]) {
    for y in 0..height {
        let t = y as f32 / height as f32;
        let r = (top[2] as f32 * (1.0 - t) + bottom[2] as f32 * t) as u8;
        let g = (top[1] as f32 * (1.0 - t) + bottom[1] as f32 * t) as u8;
        let b = (top[0] as f32 * (1.0 - t) + bottom[0] as f32 * t) as u8;
        let a = 255;
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            if idx + 3 < pixels.len() {
                pixels[idx] = b;
                pixels[idx + 1] = g;
                pixels[idx + 2] = r;
                pixels[idx + 3] = a;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_desktop_entry() {
        let content = "[Desktop Entry]\nType=Application\nName=Terminal\nExec=/bin/terminal\nIcon=terminal\nCategories=System;Terminal;\n";
        let entry = parse_desktop_entry(content).unwrap();
        assert_eq!(entry.name, "Terminal");
        assert_eq!(entry.exec, "/bin/terminal");
        assert_eq!(entry.icon, "terminal");
        assert_eq!(entry.categories, "System;Terminal;");
    }

    #[test]
    fn test_parse_non_application_type() {
        let content = "[Desktop Entry]\nType=Link\nName=File\nExec=/bin/file\n";
        assert!(parse_desktop_entry(content).is_none());
    }

    #[test]
    fn test_parse_missing_name() {
        let content = "[Desktop Entry]\nType=Application\nExec=/bin/foo\n";
        assert!(parse_desktop_entry(content).is_none());
    }

    #[test]
    fn test_parse_no_display_entry() {
        let content = "[Desktop Entry]\nType=Application\nName=Hidden\nExec=/bin/hidden\nNoDisplay=true\n";
        let entry = parse_desktop_entry(content).unwrap();
        assert!(entry.no_display);
    }

    #[test]
    fn test_parse_terminal_entry() {
        let content = "[Desktop Entry]\nType=Application\nName=CLI\nExec=/bin/cli\nTerminal=true\n";
        let entry = parse_desktop_entry(content).unwrap();
        assert!(entry.terminal);
    }

    #[test]
    fn test_parse_exec_with_args() {
        let content = "[Desktop Entry]\nType=Application\nName=Browser\nExec=/bin/browser --new-window https://example.com\n";
        let entry = parse_desktop_entry(content).unwrap();
        assert_eq!(entry.exec, "/bin/browser");
    }

    #[test]
    fn test_fill_rect_bounds_checking() {
        let mut pixels = alloc::vec![0u8; 1920 * 1080 * 4];
        fill_rect(&mut pixels, 1920, 1080, 0, 0, 2000, 2000, [255; 4]);
        let last_idx = (1920 * 1080 - 1) * 4;
        assert!(pixels[last_idx + 3] > 0);
    }
}
