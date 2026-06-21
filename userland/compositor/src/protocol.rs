//! Minimal client-server protocol for the Turnix Wayland compositor.
//!
//! Clients connect via a Unix socket at `/tmp/wayland-0` and exchange
//! fixed-size messages. This module defines the message format and
//! dispatches incoming messages to the compositor state.

use libturnix::read;

use crate::state::TurnixCompositor;

/// Opcodes for client-to-compositor messages.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClientOpcode {
    /// Create a new surface. Followed by SurfaceInfo payload.
    CreateSurface = 1,
    /// Attach a GBM buffer to a surface.
    AttachBuffer = 2,
    /// Mark a surface as damaged.
    Damage = 3,
    /// Commit pending surface state.
    Commit = 4,
    /// Set surface position on screen.
    SetPosition = 5,
    /// Map (show) a surface.
    Map = 6,
    /// Unmap (hide) a surface.
    Unmap = 7,
    /// Request the compositor to shut down.
    Quit = 8,
}

/// Opcodes for compositor-to-client messages.
#[allow(dead_code)]
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ServerOpcode {
    /// Acknowledge a create-surface request.
    SurfaceCreated = 1,
    /// Deliver a key event.
    KeyEvent = 2,
    /// Deliver a pointer motion event.
    PointerMotion = 3,
    /// Deliver a pointer button event.
    PointerButton = 4,
    /// Error response.
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

/// Parse a raw client message and apply it to the compositor.
///
/// Returns `Some(opcode)` if the message was handled, or `None` if
/// the connection should be closed (e.g. on protocol error).
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
            let _id = compositor.create_surface(client_pid, info.width, info.height);
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
            // Commit acknowledges the pending state is ready for display.
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
        _ => {
            // Unknown opcode — protocol error, close connection.
            None
        }
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
        Some(_) => true, // partial message, wait for more
        None => false,   // connection closed or error
    }
}
