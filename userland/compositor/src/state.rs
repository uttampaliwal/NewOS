extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use libturnix::{print, println, InputEvent};
use turnix_abi::input::*;

use crate::drm::DrmBackend;
use crate::input::InputManager;
use crate::protocol::{
    self, KeyEventPayload, MessageHeader, PointerButtonPayload, PointerMotionPayload,
    ServerOpcode,
};
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
    pub buffer_id: u64,
    pub damaged: bool,
    pub mapped: bool,
    pub title: Option<alloc::string::String>,
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
            title: None,
        }
    }

    pub fn clip_rect(&self, screen_w: u32, screen_h: u32) -> Option<ClipRect> {
        let sx = self.x.max(0) as u32;
        let sy = self.y.max(0) as u32;
        let ex = (self.x as i32 + self.width as i32)
            .min(screen_w as i32)
            .max(0) as u32;
        let ey = (self.y as i32 + self.height as i32)
            .min(screen_h as i32)
            .max(0) as u32;
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
    /// Maps client_pid → socket fd for message delivery.
    pub client_fds: BTreeMap<u64, u64>,
    /// The compositor's listening socket fd.
    pub listener_fd: Option<u64>,
    /// Next client PID we expect (0 = accept any).
    pub next_client_pid: u64,
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
            client_fds: BTreeMap::new(),
            listener_fd: None,
            next_client_pid: 0,
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

        let visible: Vec<Surface> = self
            .surfaces
            .values()
            .filter(|s| s.mapped)
            .cloned()
            .collect();

        composite_surfaces(gbm_phys, self.drm.width, self.drm.height, &visible);

        for id in &damaged {
            if let Some(surface) = self.surfaces.get_mut(id) {
                surface.damaged = false;
            }
        }

        let old_back = core::mem::replace(&mut self.drm.back_buffer, gbm_id);
        libturnix::drm_page_flip(gbm_id, 0);

        if old_back != 0 {
            libturnix::gbm_destroy(old_back);
        }
    }

    pub fn accept_new_clients(&mut self) {
        let listener = match self.listener_fd {
            Some(fd) => fd,
            None => return,
        };
        loop {
            match libturnix::accept(listener) {
                Some(client_fd) => {
                    // Read the PID from the first message. For now, use a simple
                    // protocol: the first message contains just the client PID as u64.
                    let mut pid_buf = [0u8; 8];
                    match libturnix::read(client_fd, &mut pid_buf) {
                        Some(8) => {
                            let pid = u64::from_ne_bytes(pid_buf);
                            self.client_fds.insert(pid, client_fd);
                        }
                        _ => {
                            // Invalid handshake — close.
                            let _ = libturnix::close(client_fd);
                        }
                    }
                }
                None => break, // No more pending connections
            }
        }
    }

    pub fn process_client_messages(&mut self) {
        let pairs: Vec<(u64, u64)> = self.client_fds.iter().map(|(&p, &f)| (p, f)).collect();
        let mut to_remove: Vec<u64> = Vec::new();
        for (pid, fd) in pairs {
            if !protocol::process_client_fd(self, pid, fd) {
                to_remove.push(pid);
                let _ = libturnix::close(fd);
            }
        }

        for pid in to_remove {
            self.client_fds.remove(&pid);
            self.remove_client_surfaces(pid);
        }
    }

    fn deliver_key_to_surface(&self, surface_id: SurfaceId, ev: &InputEvent) {
        let pid = match self.surfaces.get(&surface_id) {
            Some(s) => s.client_pid,
            None => return,
        };
        let fd = match self.client_fds.get(&pid) {
            Some(&fd) => fd,
            None => return,
        };
        let header = MessageHeader::new(
            ServerOpcode::KeyEvent as u32,
            8,
            surface_id as u32,
        );
        let payload = KeyEventPayload {
            key_code: ev.code,
            state: ev.value as u32,
            _pad: 0,
        };
        let payload_bytes =
            unsafe { core::slice::from_raw_parts(&payload as *const _ as *const u8, 8) };
        protocol::send_server_message(fd, &header, payload_bytes);
    }

    fn deliver_pointer_to_surface(&self, surface_id: SurfaceId, ev: &InputEvent) {
        let pid = match self.surfaces.get(&surface_id) {
            Some(s) => s.client_pid,
            None => return,
        };
        let fd = match self.client_fds.get(&pid) {
            Some(&fd) => fd,
            None => return,
        };

        if ev.kind == turnix_abi::input::INPUT_KIND_REL {
            // Pointer motion
            let header = MessageHeader::new(
                ServerOpcode::PointerMotion as u32,
                8,
                surface_id as u32,
            );
            let payload = PointerMotionPayload {
                x: self.input.pointer_x,
                y: self.input.pointer_y,
            };
            let payload_bytes =
                unsafe { core::slice::from_raw_parts(&payload as *const _ as *const u8, 8) };
            protocol::send_server_message(fd, &header, payload_bytes);
        } else {
            // Pointer button (left/middle/right)
            let header = MessageHeader::new(
                ServerOpcode::PointerButton as u32,
                8,
                surface_id as u32,
            );
            let payload = PointerButtonPayload {
                button: ev.code as u32,
                state: ev.value as u32,
            };
            let payload_bytes =
                unsafe { core::slice::from_raw_parts(&payload as *const _ as *const u8, 8) };
            protocol::send_server_message(fd, &header, payload_bytes);
        }
    }

    fn handle_input_event(&mut self, ev: &InputEvent) {
        match ev.kind {
            INPUT_KIND_KEY => {
                // Check if it's a pointer button (BTN_LEFT=272, BTN_RIGHT=273, BTN_MIDDLE=274)
                if ev.code == 272 || ev.code == 273 || ev.code == 274 {
                    if let Some(surface_id) =
                        self.surface_at(self.input.pointer_x, self.input.pointer_y)
                    {
                        if ev.value == 1 {
                            // Press — set focus
                            self.set_focused(surface_id);
                        }
                        self.deliver_pointer_to_surface(surface_id, ev);
                    }
                } else if let Some(focused) = self.focused {
                    self.deliver_key_to_surface(focused, ev);
                }
            }
            turnix_abi::input::INPUT_KIND_REL => {
                self.input.handle_pointer_motion(ev.code, ev.value);
                if let Some(surface_id) =
                    self.surface_at(self.input.pointer_x, self.input.pointer_y)
                {
                    self.deliver_pointer_to_surface(surface_id, ev);
                }
            }
            _ => {}
        }
    }

    pub fn tick(&mut self) {
        let mut events = [InputEvent::new(0, 0, 0); 64];
        let event_count = libturnix::input_read(&mut events);
        for i in 0..(event_count as usize) {
            let ev = &events[i];
            if ev.kind == INPUT_KIND_SYN {
                continue;
            }
            self.handle_input_event(ev);
        }

        self.accept_new_clients();
        self.process_client_messages();
        self.composite_and_flip();
    }

    pub fn run(&mut self) {
        self.drm.enable();

        // Set up Unix socket listener
        let socket_fd = match libturnix::socket(1, 1, 0) {
            Some(fd) => fd,
            None => {
                println("ERROR: cannot create compositor socket");
                return;
            }
        };

        // Remove existing socket file and bind
        let _ = libturnix::unlink(protocol::SOCKET_PATH);
        let addr_bytes = protocol::SOCKET_PATH.as_bytes();
        let mut sockaddr = [0u8; 110];
        sockaddr[0] = 1; // AF_UNIX family byte (little-endian)
        sockaddr[1] = 0;
        sockaddr[2..2 + addr_bytes.len()].copy_from_slice(addr_bytes);
        if !libturnix::bind(socket_fd, sockaddr.as_ptr(), 2 + addr_bytes.len()) {
            println("ERROR: cannot bind compositor socket");
            let _ = libturnix::close(socket_fd);
            return;
        }
        if !libturnix::listen(socket_fd, 8) {
            println("ERROR: cannot listen on compositor socket");
            let _ = libturnix::close(socket_fd);
            return;
        }

        self.listener_fd = Some(socket_fd);
        print("Compositor socket ready at ");
        println(protocol::SOCKET_PATH);

        loop {
            self.tick();
            libturnix::yielder();
        }
    }
}
