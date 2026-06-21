use libturnix::InputEvent;
use turnix_abi::input::*;

/// Tracks pointer state and keyboard focus for the compositor.
pub struct InputManager {
    pub pointer_x: i32,
    pub pointer_y: i32,
    screen_w: u32,
    screen_h: u32,
}

impl InputManager {
    pub fn new(screen_w: u32, screen_h: u32) -> Self {
        Self {
            pointer_x: (screen_w / 2) as i32,
            pointer_y: (screen_h / 2) as i32,
            screen_w,
            screen_h,
        }
    }

    pub fn handle_pointer_motion(&mut self, axis: u16, delta: i32) {
        match axis {
            REL_X => {
                self.pointer_x = (self.pointer_x + delta)
                    .max(0)
                    .min(self.screen_w as i32 - 1);
            }
            REL_Y => {
                self.pointer_y = (self.pointer_y + delta)
                    .max(0)
                    .min(self.screen_h as i32 - 1);
            }
            _ => {}
        }
    }

    /// Read pending events from the kernel and dispatch them.
    pub fn dispatch_events<F>(&mut self, mut callback: F)
    where
        F: FnMut(&InputEvent),
    {
        let mut buf = [InputEvent::new(0, 0, 0); 64];
        let count = libturnix::input_read(&mut buf);
        for i in 0..(count as usize) {
            let ev = &buf[i];
            if ev.kind == INPUT_KIND_SYN {
                continue;
            }
            callback(ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_starts_at_center() {
        let im = InputManager::new(1920, 1080);
        assert_eq!(im.pointer_x, 960);
        assert_eq!(im.pointer_y, 540);
    }

    #[test]
    fn pointer_motion_clamps_to_screen() {
        let mut im = InputManager::new(100, 100);
        im.handle_pointer_motion(REL_X, -1000);
        assert_eq!(im.pointer_x, 0);
        im.handle_pointer_motion(REL_Y, 1000);
        assert_eq!(im.pointer_y, 99);
    }

    #[test]
    fn pointer_motion_accumulates() {
        let mut im = InputManager::new(1920, 1080);
        im.handle_pointer_motion(REL_X, 10);
        im.handle_pointer_motion(REL_X, -3);
        assert_eq!(im.pointer_x, 960 + 10 - 3);
    }
}
