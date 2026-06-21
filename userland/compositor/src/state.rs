use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use libturnix::InputEvent;
use turnix_abi::input::*;

use crate::drm::DrmBackend;
use crate::input::InputManager;
use crate::render::{composite_surfaces, ClipRect};

pub type SurfaceId = u64;

/// A client surface managed by the compositor.
#[derive(Debug, Clone)]
pub struct Surface {
    pub id: SurfaceId,
    pub client_pid: u64,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub z_order: u32,
    /// GBM buffer ID backing this surface (0 = none).
    pub buffer_id: u64,
    /// Whether the surface has pending damage.
    pub damaged: bool,
    /// Whether the surface is mapped (visible).
    pub mapped: bool,
}

impl Surface {
    pub fn new(id: SurfaceId, client_pid: u64, width: u32, height: u32) -> Self {
        Self {
            id,
            client_pid,
            x: 0,
            y: 0,
            width,
            height,
            z_order: 0,
            buffer_id: 0,
            damaged: true,
            mapped: false,
        }
    }

    pub fn clip_rect(&self, screen_w: u32, screen_h: u32) -> Option<ClipRect> {
        let sx = self.x.max(0) as u32;
        let sy = self.y.max(0) as u32;
        let ex = (self.x as i32 + self.width as i32).min(screen_w as i32).max(0) as u32;
        let ey = (self.y as i32 + self.height as i32).min(screen_h as i32).max(0) as u32;
        if sx >= ex || sy >= ey {
            return None;
        }
        Some(ClipRect {
            x: sx,
            y: sy,
            width: ex - sx,
            height: ey - sy,
            src_x: if self.x < 0 { (-self.x) as u32 } else { 0 },
            src_y: if self.y < 0 { (-self.y) as u32 } else { 0 },
        })
    }
}

static NEXT_SURFACE_ID: AtomicU64 = AtomicU64::new(1);

/// The main compositor state machine.
pub struct TurnixCompositor {
    pub drm: DrmBackend,
    pub input: InputManager,
    pub surfaces: BTreeMap<SurfaceId, Surface>,
    pub focused: Option<SurfaceId>,
    pub running: bool,
}

impl TurnixCompositor {
    pub fn new() -> Option<Self> {
        let drm = DrmBackend::init()?;
        let w = drm.width;
        let h = drm.height;
        Some(Self {
            drm,
            input: InputManager::new(w, h),
            surfaces: BTreeMap::new(),
            focused: None,
            running: true,
        })
    }

    pub fn create_surface(
        &mut self,
        client_pid: u64,
        width: u32,
        height: u32,
    ) -> SurfaceId {
        let id = NEXT_SURFACE_ID.fetch_add(1, Ordering::Relaxed);
        let z = self.surfaces.len() as u32;
        let mut surface = Surface::new(id, client_pid, width, height);
        surface.z_order = z;
        self.surfaces.insert(id, surface);
        id
    }

    pub fn remove_surface(&mut self, id: SurfaceId) {
        if let Some(surface) = self.surfaces.remove(&id) {
            if surface.buffer_id != 0 {
                libturnix::gbm_destroy(surface.buffer_id);
            }
        }
        if self.focused == Some(id) {
            self.focused = None;
        }
    }

    pub fn remove_client_surfaces(&mut self, client_pid: u64) {
        let to_remove: Vec<SurfaceId> = self
            .surfaces
            .iter()
            .filter(|(_, s)| s.client_pid == client_pid)
            .map(|(id, _)| *id)
            .collect();
        for id in to_remove {
            self.remove_surface(id);
        }
    }

    pub fn surface_at(&self, x: i32, y: i32) -> Option<SurfaceId> {
        self.surfaces
            .iter()
            .filter(|(_, s)| s.mapped)
            .filter(|(_, s)| {
                x >= s.x && x < s.x + s.width as i32 && y >= s.y && y < s.y + s.height as i32
            })
            .max_by_key(|(_, s)| s.z_order)
            .map(|(id, _)| *id)
    }

    pub fn set_focused(&mut self, id: SurfaceId) {
        if !self.surfaces.contains_key(&id) {
            return;
        }
        self.focused = Some(id);
        let max_z = self.surfaces.values().map(|s| s.z_order).max().unwrap_or(0);
        if let Some(surface) = self.surfaces.get_mut(&id) {
            surface.z_order = max_z + 1;
        }
    }

    pub fn damage_surface(&mut self, id: SurfaceId) {
        if let Some(surface) = self.surfaces.get_mut(&id) {
            surface.damaged = true;
        }
    }

    pub fn set_surface_buffer(&mut self, id: SurfaceId, buffer_id: u64) {
        if let Some(surface) = self.surfaces.get_mut(&id) {
            surface.buffer_id = buffer_id;
            surface.damaged = true;
        }
    }

    pub fn set_surface_position(&mut self, id: SurfaceId, x: i32, y: i32) {
        if let Some(surface) = self.surfaces.get_mut(&id) {
            surface.x = x;
            surface.y = y;
        }
    }

    pub fn map_surface(&mut self, id: SurfaceId) {
        if let Some(surface) = self.surfaces.get_mut(&id) {
            surface.mapped = true;
            surface.damaged = true;
        }
    }

    pub fn unmap_surface(&mut self, id: SurfaceId) {
        if let Some(surface) = self.surfaces.get_mut(&id) {
            surface.mapped = false;
        }
    }

    pub fn composite_and_flip(&mut self) {
        let damaged: Vec<SurfaceId> = self
            .surfaces
            .iter()
            .filter(|(_, s)| s.mapped && s.damaged)
            .map(|(id, _)| *id)
            .collect();

        if damaged.is_empty() {
            return;
        }

        // Allocate a GBM buffer for compositing
        let gbm_id = match libturnix::gbm_create(self.drm.width, self.drm.height, 0) {
            Some(id) => id,
            None => return,
        };

        let gbm_phys = match libturnix::gbm_map(gbm_id) {
            Some(addr) => addr,
            None => {
                libturnix::gbm_destroy(gbm_id);
                return;
            }
        };

        // Composite all visible surfaces onto the buffer
        let visible: Vec<Surface> = self
            .surfaces
            .values()
            .filter(|s| s.mapped)
            .cloned()
            .collect();

        composite_surfaces(
            gbm_phys,
            self.drm.width,
            self.drm.height,
            &visible,
        );

        // Clear damage flags
        for id in &damaged {
            if let Some(surface) = self.surfaces.get_mut(id) {
                surface.damaged = false;
            }
        }

        // Old back buffer becomes stale — free it if exists
        let old_back = core::mem::replace(&mut self.drm.back_buffer, gbm_id);

        // Page flip to the new buffer
        libturnix::drm_page_flip(gbm_id, 0);

        if old_back != 0 {
            libturnix::gbm_destroy(old_back);
        }
    }

    pub fn tick(&mut self) {
        // 1. Process input events — collect into a buffer first to avoid
        //    borrowing self.input and self simultaneously.
        let mut events = [InputEvent::new(0, 0, 0); 64];
        let event_count = libturnix::input_read(&mut events);
        for i in 0..(event_count as usize) {
            let ev = &events[i];
            if ev.kind == INPUT_KIND_SYN {
                continue;
            }
            self.handle_input_event(ev);
        }

        // 2. Process client messages from accepted connections
        self.process_client_messages();

        // 3. Composite and flip if needed
        self.composite_and_flip();
    }

    fn handle_input_event(&mut self, ev: &InputEvent) {
        match ev.kind {
            INPUT_KIND_KEY => {
                if let Some(focused) = self.focused {
                    self.deliver_key_to_surface(focused, ev);
                }
            }
            turnix_abi::input::INPUT_KIND_REL => {
                self.input.handle_pointer_motion(ev.code, ev.value);
                if let Some(surface_id) = self.surface_at(self.input.pointer_x, self.input.pointer_y)
                {
                    self.deliver_pointer_to_surface(surface_id, ev);
                }
            }
            _ => {}
        }
    }

    fn deliver_key_to_surface(&self, _surface_id: SurfaceId, _ev: &InputEvent) {
        // In a full implementation, this sends the key event to the client
        // over the Wayland/Unix socket connection.
        // For now, keyboard events are acknowledged.
    }

    fn deliver_pointer_to_surface(&self, _surface_id: SurfaceId, _ev: &InputEvent) {
        // In a full implementation, this sends the pointer event to the client
        // over the Wayland/Unix socket connection.
    }

    fn process_client_messages(&mut self) {
        // In a full implementation, this reads from accepted client connections
        // and processes Wayland protocol messages (create surface, attach buffer,
        // damage, commit, etc.).
    }

    pub fn run(&mut self) {
        self.drm.enable();
        loop {
            self.tick();
            // Yield to avoid busy-waiting
            libturnix::yielder();
        }
    }
}
