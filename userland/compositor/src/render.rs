use alloc::vec::Vec;

use crate::state::Surface;

/// A rectangle describing the visible region of a surface after clipping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipRect {
    /// Screen-space X origin of the clipped region.
    pub x: u32,
    /// Screen-space Y origin of the clipped region.
    pub y: u32,
    /// Width of the clipped region in pixels.
    pub width: u32,
    /// Height of the clipped region in pixels.
    pub height: u32,
    /// Source X offset within the surface buffer.
    pub src_x: u32,
    /// Source Y offset within the surface buffer.
    pub src_y: u32,
}

/// Composite all visible surfaces onto a GBM buffer at `phys_addr`.
///
/// Surfaces are rendered in Z-order (lowest first), clipped to the display
/// bounds. Each surface's pixels are copied from its GBM buffer to the
/// output buffer. The output buffer is cleared to a background colour first.
pub fn composite_surfaces(output_phys: u64, screen_w: u32, screen_h: u32, surfaces: &[Surface]) {
    let stride = screen_w as usize * 4;
    let size = (screen_h as usize) * stride;

    let phys_mem_offset = get_phys_mem_offset();
    let out_ptr = (phys_mem_offset + output_phys) as *mut u8;

    // Clear the output buffer to dark grey (0x33, 0x33, 0x33)
    unsafe {
        core::ptr::write_bytes(out_ptr, 0x33, size);
    }

    // Sort surfaces by Z-order (ascending)
    let mut sorted: Vec<&Surface> = surfaces.iter().collect();
    sorted.sort_by_key(|s| s.z_order);

    for surface in &sorted {
        if surface.buffer_id == 0 || surface.width == 0 || surface.height == 0 {
            continue;
        }

        let clip = match surface.clip_rect(screen_w, screen_h) {
            Some(c) => c,
            None => continue,
        };

        let src_phys = match libturnix::gbm_map(surface.buffer_id) {
            Some(addr) => addr,
            None => continue,
        };
        let src_ptr = (phys_mem_offset + src_phys) as *const u8;

        let src_stride = surface.width as usize * 4;

        // Copy the visible portion of the surface into the output buffer.
        // This is a simple pixel copy (no alpha blending) for performance.
        for row in 0..clip.height {
            let src_row = (clip.src_y + row) as usize;
            let dst_row = (clip.y + row) as usize;
            let src_offset = src_row * src_stride + clip.src_x as usize * 4;
            let dst_offset = dst_row * stride + clip.x as usize * 4;
            let copy_bytes = clip.width as usize * 4;

            unsafe {
                core::ptr::copy_nonoverlapping(
                    src_ptr.add(src_offset),
                    out_ptr.add(dst_offset),
                    copy_bytes,
                );
            }
        }
    }
}

/// Get the physical memory offset provided by the bootloader.
fn get_phys_mem_offset() -> u64 {
    // The physical memory offset is stored at a well-known location by the
    // kernel at boot. On Turnix, userspace can read it from a fixed virtual
    // address or query it via a syscall.
    // For bochs-display, the framebuffer is directly accessible via the
    // physical memory offset. We use 0xFFFF_8000_0000_0000 which is the
    // standard x86-64 kernel mapping offset.
    0xFFFF_8000_0000_0000
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_surface(id: u64, x: i32, y: i32, w: u32, h: u32, z: u32, buffer_id: u64) -> Surface {
        Surface {
            id,
            client_pid: 1,
            x,
            y,
            width: w,
            height: h,
            z_order: z,
            buffer_id,
            damaged: true,
            mapped: true,
            title: None,
        }
    }

    #[test]
    fn surface_fully_inside_display() {
        let s = make_surface(1, 100, 100, 800, 600, 0, 1);
        let clip = s.clip_rect(1920, 1080);
        assert!(clip.is_some());
        let c = clip.unwrap();
        assert_eq!(c.x, 100);
        assert_eq!(c.y, 100);
        assert_eq!(c.width, 800);
        assert_eq!(c.height, 600);
        assert_eq!(c.src_x, 0);
        assert_eq!(c.src_y, 0);
    }

    #[test]
    fn surface_partially_off_left_edge() {
        let s = make_surface(1, -50, 100, 800, 600, 0, 1);
        let clip = s.clip_rect(1920, 1080);
        assert!(clip.is_some());
        let c = clip.unwrap();
        assert_eq!(c.x, 0);
        assert_eq!(c.src_x, 50);
        assert_eq!(c.width, 800 - 50);
    }

    #[test]
    fn surface_partially_off_top_edge() {
        let s = make_surface(1, 100, -30, 800, 600, 0, 1);
        let clip = s.clip_rect(1920, 1080);
        assert!(clip.is_some());
        let c = clip.unwrap();
        assert_eq!(c.y, 0);
        assert_eq!(c.src_y, 30);
        assert_eq!(c.height, 600 - 30);
    }

    #[test]
    fn surface_partially_off_right_edge() {
        let s = make_surface(1, 1800, 100, 200, 100, 0, 1);
        let clip = s.clip_rect(1920, 1080);
        assert!(clip.is_some());
        let c = clip.unwrap();
        assert_eq!(c.x, 1800);
        assert_eq!(c.width, 120);
        assert_eq!(c.src_x, 0);
    }

    #[test]
    fn surface_partially_off_bottom_edge() {
        let s = make_surface(1, 100, 1000, 800, 200, 0, 1);
        let clip = s.clip_rect(1920, 1080);
        assert!(clip.is_some());
        let c = clip.unwrap();
        assert_eq!(c.y, 1000);
        assert_eq!(c.height, 80);
        assert_eq!(c.src_y, 0);
    }

    #[test]
    fn surface_completely_off_left() {
        let s = make_surface(1, -300, 100, 200, 200, 0, 1);
        let clip = s.clip_rect(1920, 1080);
        assert!(clip.is_none());
    }

    #[test]
    fn surface_completely_off_top() {
        let s = make_surface(1, 100, -300, 200, 200, 0, 1);
        let clip = s.clip_rect(1920, 1080);
        assert!(clip.is_none());
    }

    #[test]
    fn surface_completely_off_right() {
        let s = make_surface(1, 2000, 100, 200, 200, 0, 1);
        let clip = s.clip_rect(1920, 1080);
        assert!(clip.is_none());
    }

    #[test]
    fn surface_completely_off_bottom() {
        let s = make_surface(1, 100, 1100, 200, 200, 0, 1);
        let clip = s.clip_rect(1920, 1080);
        assert!(clip.is_none());
    }

    #[test]
    fn zero_size_surface_returns_none() {
        let s = make_surface(1, 0, 0, 0, 0, 0, 1);
        let clip = s.clip_rect(1920, 1080);
        assert!(clip.is_none());
    }

    #[test]
    fn z_order_sorting() {
        // Taller surface with lower Z renders first (background)
        let background = make_surface(1, 0, 0, 100, 100, 0, 1);
        let foreground = make_surface(2, 10, 10, 50, 50, 1, 2);

        let surfaces = [foreground.clone(), background.clone()];
        let mut sorted: Vec<&Surface> = surfaces.iter().collect();
        sorted.sort_by_key(|s| s.z_order);

        assert_eq!(sorted[0].id, background.id, "background must be first");
        assert_eq!(sorted[1].id, foreground.id, "foreground must be second");
    }

    #[test]
    fn surface_at_in_bounds() {
        let s = make_surface(1, 10, 10, 100, 100, 0, 1);
        assert!(s.x <= 50 && 50 < s.x + s.width as i32);
        assert!(s.y <= 50 && 50 < s.y + s.height as i32);
    }

    #[test]
    fn surface_at_out_of_bounds() {
        let s = make_surface(1, 10, 10, 100, 100, 0, 1);
        assert!(!(200 >= s.x && 200 < s.x + s.width as i32));
    }

    #[test]
    fn clip_rect_correct_src_offset() {
        let s = make_surface(1, -20, 100, 200, 200, 0, 1);
        let clip = s.clip_rect(1920, 1080).unwrap();
        assert_eq!(clip.x, 0);
        assert_eq!(clip.src_x, 20);
        assert_eq!(clip.width, 180);
    }

    #[test]
    fn multiple_clips_correct_regions() {
        let s1 = make_surface(1, -50, 100, 400, 300, 0, 1);
        let s2 = make_surface(2, 1900, 100, 200, 300, 1, 2);

        let c1 = s1.clip_rect(1920, 1080).unwrap();
        let c2 = s2.clip_rect(1920, 1080).unwrap();

        // s1 is partially off left edge
        assert_eq!(c1.x, 0);
        assert_eq!(c1.src_x, 50);
        assert_eq!(c1.width, 350);

        // s2 is partially off right edge
        assert_eq!(c2.x, 1900);
        assert_eq!(c2.src_x, 0);
        assert_eq!(c2.width, 20);
        assert_eq!(c2.height, 300);
    }
}
