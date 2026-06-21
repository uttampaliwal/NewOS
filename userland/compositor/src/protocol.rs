use libturnix::read;

use crate::state::TurnixCompositor;

pub const SOCKET_PATH: &str = "/tmp/wayland-0";

/// Opcodes for client-to-compositor messages.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClientOpcode {
    CreateSurface = 1,
    AttachBuffer = 2,
    Damage = 3,
    Commit = 4,
    SetPosition = 5,
    Map = 6,
    Unmap = 7,
    Quit = 8,
}

/// Opcodes for compositor-to-client messages.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ServerOpcode {
    SurfaceCreated = 1,
    KeyEvent = 2,
    PointerMotion = 3,
    PointerButton = 4,
    Error = 5,
}

/// Wire-format message header (12 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MessageHeader {
    pub opcode: u32,
    pub payload_len: u32,
    pub surface_id: u32,
}

impl MessageHeader {
    pub const fn new(opcode: u32, payload_len: u32, surface_id: u32) -> Self {
        Self { opcode, payload_len, surface_id }
    }

    pub fn to_bytes(&self) -> [u8; 12] {
        unsafe { core::mem::transmute(*self) }
    }
}

/// Payload for CreateSurface.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SurfaceInfo {
    pub width: u32,
    pub height: u32,
}

/// Payload for AttachBuffer.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AttachInfo {
    pub gbm_buffer_id: u64,
}

/// Payload for SetPosition.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PositionInfo {
    pub x: i32,
    pub y: i32,
}

/// Payload for KeyEvent server message.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KeyEventPayload {
    pub key_code: u16,
    pub state: u32,
    pub _pad: u16,
}

/// Payload for PointerMotion server message.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PointerMotionPayload {
    pub x: i32,
    pub y: i32,
}

/// Payload for PointerButton server message.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PointerButtonPayload {
    pub button: u32,
    pub state: u32,
}

/// Write a server message (header + payload) to a client fd.
pub fn send_server_message(fd: u64, header: &MessageHeader, payload: &[u8]) {
    let hdr_bytes = header.to_bytes();
    let mut msg = [0u8; 128];
    msg[..12].copy_from_slice(&hdr_bytes);
    let plen = payload.len().min(128 - 12);
    msg[12..12 + plen].copy_from_slice(&payload[..plen]);
    let _ = libturnix::write(fd, &msg[..12 + plen]);
}

/// Parse a raw client message and apply it to the compositor.
pub fn handle_client_message(
    compositor: &mut TurnixCompositor,
    client_pid: u64,
    buf: &[u8],
) -> Option<ClientOpcode> {
    if buf.len() < 12 {
        return None;
    }

    let header = unsafe { &*(buf.as_ptr() as *const MessageHeader) };

    match header.opcode {
        op if op == ClientOpcode::CreateSurface as u32 => {
            if buf.len() < 12 + 8 {
                return None;
            }
            let info = unsafe { &*(buf.as_ptr().add(12) as *const SurfaceInfo) };
            let id = compositor.create_surface(client_pid, info.width, info.height);
            if let Some(&fd) = compositor.client_fds.get(&client_pid) {
                let ack = MessageHeader::new(
                    ServerOpcode::SurfaceCreated as u32,
                    0,
                    id as u32,
                );
                send_server_message(fd, &ack, &[]);
            }
            Some(ClientOpcode::CreateSurface)
        }
        op if op == ClientOpcode::AttachBuffer as u32 => {
            if buf.len() < 12 + 8 {
                return None;
            }
            let info = unsafe { &*(buf.as_ptr().add(12) as *const AttachInfo) };
            compositor.set_surface_buffer(header.surface_id as u64, info.gbm_buffer_id);
            Some(ClientOpcode::AttachBuffer)
        }
        op if op == ClientOpcode::Damage as u32 => {
            compositor.damage_surface(header.surface_id as u64);
            Some(ClientOpcode::Damage)
        }
        op if op == ClientOpcode::Commit as u32 => {
            compositor.composite_and_flip();
            Some(ClientOpcode::Commit)
        }
        op if op == ClientOpcode::SetPosition as u32 => {
            if buf.len() < 12 + 8 {
                return None;
            }
            let info = unsafe { &*(buf.as_ptr().add(12) as *const PositionInfo) };
            compositor.set_surface_position(header.surface_id as u64, info.x, info.y);
            Some(ClientOpcode::SetPosition)
        }
        op if op == ClientOpcode::Map as u32 => {
            compositor.map_surface(header.surface_id as u64);
            Some(ClientOpcode::Map)
        }
        op if op == ClientOpcode::Unmap as u32 => {
            compositor.unmap_surface(header.surface_id as u64);
            Some(ClientOpcode::Unmap)
        }
        op if op == ClientOpcode::Quit as u32 => {
            compositor.running = false;
            Some(ClientOpcode::Quit)
        }
        _ => None,
    }
}

/// Attempt to read and process one message from a client socket.
pub fn process_client_fd(compositor: &mut TurnixCompositor, client_pid: u64, fd: u64) -> bool {
    let mut buf = [0u8; 128];
    match read(fd, &mut buf) {
        Some(n) if n >= 12 => {
            let data = &buf[..n as usize];
            handle_client_message(compositor, client_pid, data).is_some()
        }
        Some(_) => true,
        None => false,
    }
}
