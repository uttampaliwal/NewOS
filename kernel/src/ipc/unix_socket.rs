use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

use crate::ipc::pipe::PipeBuffer;

// ---------------------------------------------------------------------------
// Global registry: filesystem path  →  listening `Arc<UnixSocketState>`
// ---------------------------------------------------------------------------

lazy_static! {
    static ref BOUND_SOCKETS: Mutex<BTreeMap<String, Arc<UnixSocketState>>> =
        Mutex::new(BTreeMap::new());
}

fn register_bind(path: &str, sock: &Arc<UnixSocketState>) -> Result<(), ()> {
    let mut map = BOUND_SOCKETS.lock();
    if map.contains_key(path) {
        return Err(());
    }
    map.insert(String::from(path), sock.clone());
    Ok(())
}

fn unregister_bind(path: &str) {
    BOUND_SOCKETS.lock().remove(path);
}

fn lookup_bind(path: &str) -> Option<Arc<UnixSocketState>> {
    BOUND_SOCKETS.lock().get(path).cloned()
}

// ---------------------------------------------------------------------------
// Connection — shared state between two connected endpoints
// ---------------------------------------------------------------------------

/// Bidirectional byte buffers shared by a connected pair.
pub struct Connection {
    /// From endpoint-0 to endpoint-1.
    pub buf_0_to_1: PipeBuffer,
    /// From endpoint-1 to endpoint-0.
    pub buf_1_to_0: PipeBuffer,
    /// True while endpoint-0 is open.
    pub open_0: AtomicBool,
    /// True while endpoint-1 is open.
    pub open_1: AtomicBool,
    /// Reference to the other endpoint's open flag.
    /// Endpoint-0's `other` points to `open_1`, and vice-versa.
    pub other_open: AtomicBool,
}

/// One side of a connected Unix socket pair.
pub struct ConnectedEnd {
    pub connection: Arc<Connection>,
    /// Index of this endpoint (0 or 1).
    pub index: u8,
}

impl ConnectedEnd {
    fn new_pair() -> (Self, Self) {
        let conn = Arc::new(Connection {
            buf_0_to_1: PipeBuffer::new(),
            buf_1_to_0: PipeBuffer::new(),
            open_0: AtomicBool::new(true),
            open_1: AtomicBool::new(true),
            other_open: AtomicBool::new(true),
        });
        let end0 = ConnectedEnd {
            connection: conn.clone(),
            index: 0,
        };
        let end1 = ConnectedEnd {
            connection: conn,
            index: 1,
        };
        (end0, end1)
    }

    fn rx(&self) -> &PipeBuffer {
        if self.index == 0 {
            &self.connection.buf_1_to_0
        } else {
            &self.connection.buf_0_to_1
        }
    }

    fn tx(&self) -> &PipeBuffer {
        if self.index == 0 {
            &self.connection.buf_0_to_1
        } else {
            &self.connection.buf_1_to_0
        }
    }

    fn my_open(&self) -> &AtomicBool {
        if self.index == 0 {
            &self.connection.open_0
        } else {
            &self.connection.open_1
        }
    }

    fn other_open(&self) -> &AtomicBool {
        if self.index == 0 {
            &self.connection.open_1
        } else {
            &self.connection.open_0
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> usize {
        self.rx().read(buf)
    }

    pub fn write(&self, buf: &[u8]) -> usize {
        self.tx().write(buf)
    }

    pub fn bytes_available(&self) -> usize {
        self.rx().bytes_available()
    }

    pub fn is_peer_open(&self) -> bool {
        self.other_open().load(Ordering::Acquire)
    }

    /// Mark this endpoint as closed and signal the peer.
    pub fn shutdown(&self) {
        self.my_open().store(false, Ordering::Release);
        self.tx().close_write_end();
        self.rx().close_read_end();
    }
}

// ---------------------------------------------------------------------------
// Internal state enum
// ---------------------------------------------------------------------------

enum SocketState {
    Idle,
    Listening {
        path: String,
        backlog: usize,
        pending: Vec<ConnectedEnd>,
    },
    Connected(ConnectedEnd),
}

// ---------------------------------------------------------------------------
// UnixSocketState — public API
// ---------------------------------------------------------------------------

/// Per-socket mutable state for a Unix domain socket.
pub struct UnixSocketState {
    inner: Mutex<SocketState>,
}

impl Default for UnixSocketState {
    fn default() -> Self {
        Self::new()
    }
}

impl UnixSocketState {
    pub fn new() -> Self {
        UnixSocketState {
            inner: Mutex::new(SocketState::Idle),
        }
    }

    /// Returns (rx_bytes_available, tx_space_available) for epoll-like polling.
    /// Returns (0, 0) if not connected.
    pub fn poll(&self) -> (usize, usize) {
        let inner = self.inner.lock();
        match &*inner {
            SocketState::Connected(conn) => {
                let rx = conn.rx();
                let tx = conn.tx();
                (rx.bytes_available(), tx.space_available())
            }
            SocketState::Listening { .. } => (1, 0),
            SocketState::Idle => (0, 0),
        }
    }

    /// Bind this socket to `path` and start listening.
    #[allow(clippy::result_unit_err)]
    pub fn bind(this: &Arc<Self>, path: &str) -> Result<String, ()> {
        if path.is_empty() {
            return Err(());
        }
        let mut inner = this.inner.lock();
        match &*inner {
            SocketState::Idle => {}
            _ => return Err(()),
        }
        register_bind(path, this)?;
        *inner = SocketState::Listening {
            path: String::from(path),
            backlog: 5,
            pending: Vec::new(),
        };
        Ok(String::from(path))
    }

    /// Set (or update) the listen backlog.
    #[allow(clippy::result_unit_err)]
    pub fn listen(this: &Arc<Self>, backlog: usize) -> Result<(), ()> {
        let mut inner = this.inner.lock();
        match &mut *inner {
            SocketState::Listening { backlog: b, .. } => {
                *b = backlog;
                Ok(())
            }
            _ => Err(()),
        }
    }

    /// Accept the oldest pending connection.
    pub fn accept(this: &Arc<Self>) -> Option<ConnectedEnd> {
        let mut inner = this.inner.lock();
        match &mut *inner {
            SocketState::Listening { pending, .. } => {
                if pending.is_empty() {
                    return None;
                }
                Some(pending.remove(0))
            }
            _ => None,
        }
    }

    /// Connect to a listening socket at `path`.
    #[allow(clippy::result_unit_err)]
    pub fn connect(this: &Arc<Self>, path: &str) -> Result<(), ()> {
        let server_sock = lookup_bind(path).ok_or(())?;

        let (server_end, client_end) = ConnectedEnd::new_pair();

        {
            let mut server_inner = server_sock.inner.lock();
            match &mut *server_inner {
                SocketState::Listening { pending, .. } => {
                    pending.push(server_end);
                }
                _ => return Err(()),
            }
        }

        let mut inner = this.inner.lock();
        match &*inner {
            SocketState::Idle => {
                *inner = SocketState::Connected(client_end);
                Ok(())
            }
            _ => Err(()),
        }
    }

    /// Read from the connected socket (non-blocking).
    pub fn read(&self, buf: &mut [u8]) -> usize {
        let inner = self.inner.lock();
        match &*inner {
            SocketState::Connected(end) => end.read(buf),
            _ => 0,
        }
    }

    /// Write to the connected socket (non-blocking).
    pub fn write(&self, buf: &[u8]) -> usize {
        let inner = self.inner.lock();
        match &*inner {
            SocketState::Connected(end) => end.write(buf),
            _ => 0,
        }
    }

    /// Bytes available to read.
    pub fn bytes_available(&self) -> usize {
        let inner = self.inner.lock();
        match &*inner {
            SocketState::Connected(end) => end.bytes_available(),
            _ => 0,
        }
    }

    /// Notify the peer that this end is closed and clean up bound state.
    pub fn shutdown(&self) {
        let mut inner = self.inner.lock();
        let old = core::mem::replace(&mut *inner, SocketState::Idle);
        match old {
            SocketState::Listening { path, .. } => {
                unregister_bind(&path);
            }
            SocketState::Connected(end) => {
                end.shutdown();
            }
            SocketState::Idle => {}
        }
    }

    /// Wrap an existing `ConnectedEnd` in a new `UnixSocketState`.
    pub fn from_connected_end(end: ConnectedEnd) -> Self {
        UnixSocketState {
            inner: Mutex::new(SocketState::Connected(end)),
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(*self.inner.lock(), SocketState::Connected(_))
    }

    pub fn is_listening(&self) -> bool {
        matches!(*self.inner.lock(), SocketState::Listening { .. })
    }

    /// Whether the peer has closed its end.
    pub fn is_peer_closed(&self) -> bool {
        let inner = self.inner.lock();
        match &*inner {
            SocketState::Connected(end) => !end.is_peer_open(),
            _ => true,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Helper: create a fresh listening socket at a unique path.
    fn create_listener(path: &str) -> Arc<UnixSocketState> {
        let sock = Arc::new(UnixSocketState::new());
        UnixSocketState::bind(&sock, path).unwrap();
        UnixSocketState::listen(&sock, 5).unwrap();
        sock
    }

    // ------------------------------------------------------------------
    // Property 18 — Unix Socket Data Integrity
    // ------------------------------------------------------------------
    //
    // For any arbitrary byte sequence:
    //   1. A client connects to a listening server socket.
    //   2. The server accepts the connection.
    //   3. Data written by the client is readable by the server,
    //      and data written by the server is readable by the client.
    //
    // Validates Requirements 19.1.

    proptest! {
        #[test]
        fn unix_socket_data_integrity(
            server_msg in proptest::collection::vec(0u8..=255, 0..=256),
            client_msg in proptest::collection::vec(0u8..=255, 0..=256),
            path_suffix in "[a-z]{8}",
        ) {
            let path = alloc::format!("/tmp/test_socket_{}", path_suffix);
            let listener = create_listener(&path);

            let client = Arc::new(UnixSocketState::new());
            UnixSocketState::connect(&client, &path).unwrap();

            // Server accepts.
            let server_end = UnixSocketState::accept(&listener).unwrap();
            let server = Arc::new(UnixSocketState::from_connected_end(server_end));

            // Server writes, client reads.
            let swritten = server.write(&server_msg);
            prop_assert_eq!(swritten, server_msg.len());

            let mut client_buf = alloc::vec![0u8; server_msg.len()];
            let cread = client.read(&mut client_buf);
            prop_assert_eq!(cread, server_msg.len());
            prop_assert_eq!(&client_buf[..cread], &server_msg[..]);

            // Client writes, server reads.
            let cwritten = client.write(&client_msg);
            prop_assert_eq!(cwritten, client_msg.len());

            let mut server_buf = alloc::vec![0u8; client_msg.len()];
            let sread = server.read(&mut server_buf);
            prop_assert_eq!(sread, client_msg.len());
            prop_assert_eq!(&server_buf[..sread], &client_msg[..]);
        }
    }

    #[test]
    fn socket_bind_creates_registry_entry() {
        let path = "/tmp/test_bind_registry";
        let sock = Arc::new(UnixSocketState::new());
        UnixSocketState::bind(&sock, path).unwrap();
        UnixSocketState::listen(&sock, 5).unwrap();
        let found = lookup_bind(path);
        assert!(
            found.is_some(),
            "socket must be in BOUND_SOCKETS after bind"
        );
    }

    #[test]
    fn socket_connect_nonexistent_path_fails() {
        let client = Arc::new(UnixSocketState::new());
        let result = UnixSocketState::connect(&client, "/tmp/nonexistent_socket");
        assert!(
            result.is_err(),
            "connect to non-existent path must return Err"
        );
    }

    #[test]
    fn socket_accept_returns_connected_endpoint() {
        let path = "/tmp/test_accept_connect";
        let listener = create_listener(path);

        let client = Arc::new(UnixSocketState::new());
        UnixSocketState::connect(&client, path).unwrap();

        // Server accepts.
        let accepted = UnixSocketState::accept(&listener);
        assert!(
            accepted.is_some(),
            "accept must return a connected endpoint"
        );

        let accepted = accepted.unwrap();
        assert!(accepted.is_peer_open(), "peer must be open after accept");
    }

    #[test]
    fn socket_bind_twice_same_path_fails() {
        let path = "/tmp/test_bind_twice";
        let sock1 = Arc::new(UnixSocketState::new());
        UnixSocketState::bind(&sock1, path).unwrap();

        let sock2 = Arc::new(UnixSocketState::new());
        let result = UnixSocketState::bind(&sock2, path);
        assert!(result.is_err(), "bind same path twice must fail");
    }

    #[test]
    fn socket_write_read_roundtrip() {
        let path = "/tmp/test_write_read_rt";
        let listener = create_listener(path);

        let client = Arc::new(UnixSocketState::new());
        UnixSocketState::connect(&client, path).unwrap();

        let server_end = UnixSocketState::accept(&listener).unwrap();
        let server = Arc::new(UnixSocketState::from_connected_end(server_end));

        let msg = b"hello unix socket";
        let written = client.write(msg);
        assert_eq!(written, msg.len());

        let mut buf = [0u8; 64];
        let n = server.read(&mut buf);
        assert_eq!(n, msg.len());
        assert_eq!(&buf[..n], msg);
    }

    #[test]
    fn socket_shutdown_signals_peer() {
        let path = "/tmp/test_shutdown";
        let listener = create_listener(path);

        let client = Arc::new(UnixSocketState::new());
        UnixSocketState::connect(&client, path).unwrap();

        let server_end = UnixSocketState::accept(&listener).unwrap();
        let server = Arc::new(UnixSocketState::from_connected_end(server_end));

        // Write some data from server.
        server.write(b"data");

        // Shutdown the client.
        client.shutdown();

        // Server should see peer as closed.
        assert!(server.is_peer_closed(), "server must detect peer close");
    }

    #[test]
    fn socket_accept_empty_queue_returns_none() {
        let path = "/tmp/test_accept_empty";
        let listener = create_listener(path);
        let result = UnixSocketState::accept(&listener);
        assert!(result.is_none(), "accept on empty queue must return None");
    }

    #[test]
    fn socket_connect_to_non_listener_fails() {
        let path = "/tmp/test_non_listener";
        // Create a socket but do NOT call bind (so it stays Idle).
        let not_listening = Arc::new(UnixSocketState::new());
        // Manually register it in the bound-sockets map without setting Listening state.
        {
            let mut map = BOUND_SOCKETS.lock();
            map.insert(String::from(path), not_listening.clone());
        }
        // Connect to the registered-but-not-listening socket.
        let client = Arc::new(UnixSocketState::new());
        let result = UnixSocketState::connect(&client, path);
        assert!(result.is_err(), "connect to non-listener must fail");
        // Clean up.
        unregister_bind(path);
    }

    #[test]
    fn socket_read_not_connected_returns_zero() {
        let sock = Arc::new(UnixSocketState::new());
        let mut buf = [0u8; 16];
        let n = sock.read(&mut buf);
        assert_eq!(n, 0, "read from unconnected socket must return 0");
    }

    #[test]
    fn socket_write_not_connected_returns_zero() {
        let sock = Arc::new(UnixSocketState::new());
        let n = sock.write(b"hello");
        assert_eq!(n, 0, "write to unconnected socket must return 0");
    }

    #[test]
    fn socket_bytes_available_not_connected() {
        let sock = Arc::new(UnixSocketState::new());
        assert_eq!(sock.bytes_available(), 0);
    }

    #[test]
    fn socket_bind_empty_path_fails() {
        let sock = Arc::new(UnixSocketState::new());
        assert!(UnixSocketState::bind(&sock, "").is_err());
    }

    #[test]
    fn socket_multiple_connections_accepted() {
        let path = "/tmp/test_multiple_accept";
        let listener = create_listener(path);

        // Connect 3 clients
        let c1 = Arc::new(UnixSocketState::new());
        UnixSocketState::connect(&c1, path).unwrap();
        let c2 = Arc::new(UnixSocketState::new());
        UnixSocketState::connect(&c2, path).unwrap();
        let c3 = Arc::new(UnixSocketState::new());
        UnixSocketState::connect(&c3, path).unwrap();

        // Accept all 3
        for _ in 0..3 {
            let accepted = UnixSocketState::accept(&listener);
            assert!(accepted.is_some(), "must accept all connections");
        }

        // No more pending
        assert!(UnixSocketState::accept(&listener).is_none());
    }
}
