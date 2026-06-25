use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use smoltcp::iface::SocketHandle;
use smoltcp::wire::{IpAddress, IpEndpoint};
use spin::Mutex;

use crate::net::smoltcp_iface::NET_STACK;

// ---------------------------------------------------------------------------
// Error codes
// ---------------------------------------------------------------------------

pub const ESUCCESS: i64 = 0;
pub const EPERM: i64 = 1;
pub const ENOENT: i64 = 2;
pub const EINTR: i64 = 4;
pub const EIO: i64 = 5;
pub const EBADF: i64 = 9;
pub const EAGAIN: i64 = 11;
pub const ENOMEM: i64 = 12;
pub const EACCES: i64 = 13;
pub const EFAULT: i64 = 14;
pub const EINVAL: i64 = 22;
pub const EADDRINUSE: i64 = 48;
pub const ECONNREFUSED: i64 = 61;
pub const ECONNRESET: i64 = 104;
pub const EAFNOSUPPORT: i64 = 97;
pub const EPROTONOSUPPORT: i64 = 95;
pub const ENOTSOCK: i64 = 88;
pub const EOPNOTSUPP: i64 = 95;
pub const ETIMEDOUT: i64 = 110;

// ---------------------------------------------------------------------------
// Socket type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetSocketType {
    Tcp,
    Udp,
}

// ---------------------------------------------------------------------------
// Socket metadata stored per FD
// ---------------------------------------------------------------------------

pub struct NetSocketEntry {
    pub handle: SocketHandle,
    pub sock_type: NetSocketType,
    pub domain: i32,
    pub local_port: Option<u16>,
    pub remote_endpoint: Option<IpEndpoint>,
    pub is_bound: bool,
    pub is_listening: bool,
    pub is_connected: bool,
    pub backlog: usize,
}

impl NetSocketEntry {
    pub fn new(handle: SocketHandle, sock_type: NetSocketType, domain: i32) -> Self {
        Self {
            handle,
            sock_type,
            domain,
            local_port: None,
            remote_endpoint: None,
            is_bound: false,
            is_listening: false,
            is_connected: false,
            backlog: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Socket table: maps FD numbers to network sockets
// ---------------------------------------------------------------------------

pub struct SocketTable {
    fds: BTreeMap<usize, NetSocketEntry>,
    next_fd: usize,
}

impl Default for SocketTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SocketTable {
    pub fn new() -> Self {
        Self {
            fds: BTreeMap::new(),
            next_fd: 256, // Start above reserved FDs (0-2 are stdio)
        }
    }

    pub fn alloc_fd(&mut self) -> Option<usize> {
        let fd = self.next_fd;
        if fd >= 4096 {
            return None; // too many open sockets
        }
        self.next_fd += 1;
        Some(fd)
    }

    pub fn insert(&mut self, entry: NetSocketEntry) -> Option<usize> {
        let fd = self.alloc_fd()?;
        self.fds.insert(fd, entry);
        Some(fd)
    }

    pub fn get(&self, fd: usize) -> Option<&NetSocketEntry> {
        self.fds.get(&fd)
    }

    pub fn get_mut(&mut self, fd: usize) -> Option<&mut NetSocketEntry> {
        self.fds.get_mut(&fd)
    }

    pub fn remove(&mut self, fd: usize) -> Option<NetSocketEntry> {
        self.fds.remove(&fd)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&usize, &NetSocketEntry)> {
        self.fds.iter()
    }
}

// ---------------------------------------------------------------------------
// Global socket table
// ---------------------------------------------------------------------------

lazy_static::lazy_static! {
    pub static ref SOCKET_TABLE: Arc<Mutex<SocketTable>> = Arc::new(Mutex::new(SocketTable::new()));
}

// ---------------------------------------------------------------------------
// Socket syscall helpers
// ---------------------------------------------------------------------------

/// Create a new socket FD. Returns the FD number on success.
pub fn sys_socket(domain: i32, sock_type: i32) -> Result<usize, i64> {
    // Only AF_INET and AF_UNIX are supported
    if domain != 2 && domain != 1 {
        return Err(EAFNOSUPPORT);
    }

    let net_type = match sock_type {
        1 => NetSocketType::Tcp, // SOCK_STREAM
        2 => NetSocketType::Udp, // SOCK_DGRAM
        _ => return Err(EPROTONOSUPPORT),
    };

    let mut stack = NET_STACK.lock();
    let handle = match net_type {
        NetSocketType::Tcp => stack.add_tcp_socket(),
        NetSocketType::Udp => stack.add_udp_socket(),
    };

    let mut table = SOCKET_TABLE.lock();
    let entry = NetSocketEntry::new(handle, net_type, domain);
    match table.insert(entry) {
        Some(fd) => Ok(fd),
        None => {
            stack.remove_socket(handle);
            Err(ENOMEM)
        }
    }
}

/// Close a socket FD.
pub fn sys_close(fd: usize) -> Result<(), i64> {
    let mut table = SOCKET_TABLE.lock();
    if let Some(entry) = table.remove(fd) {
        let mut stack = NET_STACK.lock();
        stack.remove_socket(entry.handle);
        Ok(())
    } else {
        Err(EBADF)
    }
}

/// Bind a socket to a local address.
pub fn sys_bind(fd: usize, addr: IpAddress, port: u16) -> Result<(), i64> {
    let handle;
    let sock_type;
    {
        let table = SOCKET_TABLE.lock();
        let entry = table.get(fd).ok_or(EBADF)?;
        if entry.is_bound {
            return Err(EINVAL);
        }
        handle = entry.handle;
        sock_type = entry.sock_type;
    }

    let mut stack = NET_STACK.lock();
    match sock_type {
        NetSocketType::Tcp => {
            let socket = stack
                .sockets
                .get_mut::<smoltcp::socket::tcp::Socket>(handle);
            let endpoint = IpEndpoint::new(addr, port);
            socket.listen(endpoint).map_err(|_| EADDRINUSE)?;
        }
        NetSocketType::Udp => {
            let socket = stack
                .sockets
                .get_mut::<smoltcp::socket::udp::Socket>(handle);
            let endpoint = IpEndpoint::new(addr, port);
            socket.bind(endpoint).map_err(|_| EADDRINUSE)?;
        }
    }

    let mut table = SOCKET_TABLE.lock();
    if let Some(entry) = table.get_mut(fd) {
        entry.is_bound = true;
        entry.local_port = Some(port);
    }

    Ok(())
}

/// Listen on a bound TCP socket.
pub fn sys_listen(fd: usize, backlog: usize) -> Result<(), i64> {
    let mut table = SOCKET_TABLE.lock();
    let entry = table.get_mut(fd).ok_or(EBADF)?;

    if entry.sock_type != NetSocketType::Tcp {
        return Err(EOPNOTSUPP);
    }

    if !entry.is_bound || entry.local_port.is_none() {
        return Err(EINVAL);
    }

    entry.is_listening = true;
    entry.backlog = backlog;

    Ok(())
}

/// Connect a TCP socket to a remote endpoint.
pub fn sys_connect(fd: usize, addr: IpAddress, port: u16) -> Result<(), i64> {
    let handle;
    let sock_type;
    {
        let table = SOCKET_TABLE.lock();
        let entry = table.get(fd).ok_or(EBADF)?;
        if entry.is_listening || entry.is_connected {
            return Err(EINVAL);
        }
        handle = entry.handle;
        sock_type = entry.sock_type;
    }

    let mut stack = NET_STACK.lock();
    match sock_type {
        NetSocketType::Tcp => {
            let endpoint = IpEndpoint::new(addr, port);
            stack
                .connect_tcp(handle, endpoint)
                .map_err(|_| ECONNREFUSED)?;
        }
        NetSocketType::Udp => {
            // UDP is connectionless, but we store the default remote endpoint
        }
    }

    let mut table = SOCKET_TABLE.lock();
    if let Some(entry) = table.get_mut(fd) {
        entry.is_connected = true;
        entry.remote_endpoint = Some(IpEndpoint::new(addr, port));
    }

    Ok(())
}

/// Accept a new TCP connection. Returns the new FD.
pub fn sys_accept(fd: usize) -> Result<usize, i64> {
    let handle;
    let local_port;
    {
        let table = SOCKET_TABLE.lock();
        let entry = table.get(fd).ok_or(EBADF)?;
        if entry.sock_type != NetSocketType::Tcp {
            return Err(EOPNOTSUPP);
        }
        if !entry.is_listening {
            return Err(EINVAL);
        }
        handle = entry.handle;
        local_port = entry.local_port;
    }

    let local_port = local_port.ok_or(EINVAL)?;

    let mut stack = NET_STACK.lock();

    let listener = stack
        .sockets
        .get_mut::<smoltcp::socket::tcp::Socket>(handle);

    if !listener.is_open() {
        return Err(ECONNRESET);
    }

    match listener.state() {
        smoltcp::socket::tcp::State::Listen => Err(EAGAIN),
        smoltcp::socket::tcp::State::SynReceived | smoltcp::socket::tcp::State::Established => {
            // The listening socket has transitioned to an established connection.
            // We need to:
            // 1. Create a new listener socket to replace it on the original FD
            // 2. Allocate a new FD for the accepted (now established) connection

            let new_listener_handle = stack.add_tcp_socket();
            {
                let new_listener = stack
                    .sockets
                    .get_mut::<smoltcp::socket::tcp::Socket>(new_listener_handle);
                let local = (smoltcp::wire::IpAddress::v4(0, 0, 0, 0), local_port);
                if new_listener.listen(local).is_err() {
                    stack.remove_socket(new_listener_handle);
                    return Err(EINVAL);
                }
            }

            drop(stack);

            // Allocate a new FD for the accepted connection (old handle)
            let new_fd = {
                let mut table = SOCKET_TABLE.lock();
                let accepted_entry = NetSocketEntry {
                    handle,
                    sock_type: NetSocketType::Tcp,
                    domain: 2,
                    local_port: Some(local_port),
                    remote_endpoint: None,
                    is_bound: true,
                    is_listening: false,
                    is_connected: true,
                    backlog: 0,
                };
                table.insert(accepted_entry).ok_or(ENOMEM)?
            };

            // Update the original FD to use the new listener socket
            {
                let mut table = SOCKET_TABLE.lock();
                if let Some(entry) = table.get_mut(fd) {
                    entry.handle = new_listener_handle;
                    entry.is_connected = false;
                    entry.is_listening = true;
                }
            }

            Ok(new_fd)
        }
        _ => Err(EAGAIN),
    }
}

/// Read from a TCP socket into a buffer. Returns bytes read.
pub fn sys_recv(fd: usize, buf: &mut [u8]) -> Result<usize, i64> {
    let handle;
    let sock_type;
    {
        let table = SOCKET_TABLE.lock();
        let entry = table.get(fd).ok_or(EBADF)?;
        handle = entry.handle;
        sock_type = entry.sock_type;
    }

    let mut stack = NET_STACK.lock();
    match sock_type {
        NetSocketType::Tcp => {
            let socket = stack
                .sockets
                .get_mut::<smoltcp::socket::tcp::Socket>(handle);
            if !socket.may_recv() {
                return Err(EAGAIN);
            }
            socket.recv_slice(buf).map_err(|e| match e {
                smoltcp::socket::tcp::RecvError::InvalidState => EAGAIN,
                smoltcp::socket::tcp::RecvError::Finished => EIO,
            })
        }
        NetSocketType::Udp => {
            let socket = stack
                .sockets
                .get_mut::<smoltcp::socket::udp::Socket>(handle);
            if !socket.can_recv() {
                return Err(EAGAIN);
            }
            let (data, _meta) = socket.recv().map_err(|_| EAGAIN)?;
            let len = data.len().min(buf.len());
            buf[..len].copy_from_slice(&data[..len]);
            Ok(len)
        }
    }
}

/// Write to a TCP socket from a buffer. Returns bytes written.
pub fn sys_send(fd: usize, buf: &[u8]) -> Result<usize, i64> {
    let handle;
    let sock_type;
    {
        let table = SOCKET_TABLE.lock();
        let entry = table.get(fd).ok_or(EBADF)?;
        handle = entry.handle;
        sock_type = entry.sock_type;
    }

    let mut stack = NET_STACK.lock();
    match sock_type {
        NetSocketType::Tcp => {
            let socket = stack
                .sockets
                .get_mut::<smoltcp::socket::tcp::Socket>(handle);
            if !socket.may_send() {
                return Err(EAGAIN);
            }
            socket.send_slice(buf).map_err(|e| match e {
                smoltcp::socket::tcp::SendError::InvalidState => EIO,
            })
        }
        NetSocketType::Udp => {
            let socket = stack
                .sockets
                .get_mut::<smoltcp::socket::udp::Socket>(handle);
            // For UDP, we need a remote endpoint
            let table = SOCKET_TABLE.lock();
            let entry = table.get(fd).ok_or(EBADF)?;
            let remote = entry.remote_endpoint.ok_or(EINVAL)?;
            drop(table);
            socket.send_slice(buf, remote).map_err(|_| EIO)?;
            Ok(buf.len())
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_table_empty() {
        let table = SocketTable::new();
        assert!(table.fds.is_empty());
    }

    #[test]
    fn test_socket_table_alloc_and_insert() {
        let mut stack = NET_STACK.lock();
        let handle = stack.add_tcp_socket();
        let mut table = SocketTable::new();
        let entry = NetSocketEntry::new(handle, NetSocketType::Tcp, 2);
        let fd = table.insert(entry).unwrap();
        assert!(fd >= 256);
        assert!(table.get(fd).is_some());
        drop(stack); // release lock
    }

    #[test]
    fn test_socket_table_remove() {
        let mut stack = NET_STACK.lock();
        let handle = stack.add_tcp_socket();
        let mut table = SocketTable::new();
        let entry = NetSocketEntry::new(handle, NetSocketType::Tcp, 2);
        let fd = table.insert(entry).unwrap();
        assert!(table.remove(fd).is_some());
        assert!(table.get(fd).is_none());
        drop(stack);
    }

    #[test]
    fn test_socket_table_get_mut() {
        let mut stack = NET_STACK.lock();
        let handle = stack.add_tcp_socket();
        let mut table = SocketTable::new();
        let entry = NetSocketEntry::new(handle, NetSocketType::Tcp, 2);
        let fd = table.insert(entry).unwrap();
        {
            let entry = table.get_mut(fd).unwrap();
            entry.is_bound = true;
        }
        assert!(table.get(fd).unwrap().is_bound);
        drop(stack);
    }

    #[test]
    fn test_socket_table_multiple_fds() {
        let mut stack = NET_STACK.lock();
        let h1 = stack.add_tcp_socket();
        let h2 = stack.add_udp_socket();
        let h3 = stack.add_tcp_socket();
        let mut table = SocketTable::new();
        let fd1 = table
            .insert(NetSocketEntry::new(h1, NetSocketType::Tcp, 2))
            .unwrap();
        let fd2 = table
            .insert(NetSocketEntry::new(h2, NetSocketType::Udp, 2))
            .unwrap();
        let fd3 = table
            .insert(NetSocketEntry::new(h3, NetSocketType::Tcp, 2))
            .unwrap();
        assert!(fd2 > fd1);
        assert!(fd3 > fd2);
        assert_eq!(table.fds.len(), 3);
        drop(stack);
    }

    #[test]
    fn test_net_socket_entry_defaults() {
        let mut stack = NET_STACK.lock();
        let handle = stack.add_tcp_socket();
        let entry = NetSocketEntry::new(handle, NetSocketType::Udp, 2);
        assert!(!entry.is_bound);
        assert!(!entry.is_listening);
        assert!(!entry.is_connected);
        assert!(entry.local_port.is_none());
        drop(stack);
    }

    #[test]
    fn test_socket_table_new_fds_starts_above_255() {
        let mut stack = NET_STACK.lock();
        let mut table = SocketTable::new();
        for _i in 0..10 {
            let handle = stack.add_udp_socket();
            let fd = table
                .insert(NetSocketEntry::new(handle, NetSocketType::Tcp, 2))
                .unwrap();
            assert!(fd >= 256, "FD {} should be >= 256", fd);
        }
        drop(stack);
    }

    #[test]
    fn test_socket_table_alloc_fd_exhaustion_returns_none() {
        let mut stack = NET_STACK.lock();
        let handle = stack.add_tcp_socket();
        let mut table = SocketTable::new();
        table.next_fd = 4096;

        assert!(table.alloc_fd().is_none());
        assert!(
            table
                .insert(NetSocketEntry::new(handle, NetSocketType::Tcp, 2))
                .is_none()
        );
        drop(stack);
    }

    // -----------------------------------------------------------------------
    // Additional socket table and metadata tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_socket_table_iter_returns_all_entries() {
        let mut stack = NET_STACK.lock();
        let h1 = stack.add_tcp_socket();
        let h2 = stack.add_udp_socket();
        let mut table = SocketTable::new();
        let fd1 = table
            .insert(NetSocketEntry::new(h1, NetSocketType::Tcp, 2))
            .unwrap();
        let fd2 = table
            .insert(NetSocketEntry::new(h2, NetSocketType::Udp, 2))
            .unwrap();
        let fds: alloc::vec::Vec<usize> = table.iter().map(|(fd, _)| *fd).collect();
        assert!(fds.contains(&fd1));
        assert!(fds.contains(&fd2));
        assert_eq!(fds.len(), 2);
        drop(stack);
    }

    #[test]
    fn test_net_socket_entry_with_backlog() {
        let mut stack = NET_STACK.lock();
        let handle = stack.add_tcp_socket();
        let mut entry = NetSocketEntry::new(handle, NetSocketType::Tcp, 2);
        entry.backlog = 128;
        entry.is_listening = true;
        assert_eq!(entry.backlog, 128);
        assert!(entry.is_listening);
        drop(stack);
    }

    #[test]
    fn test_socket_table_remove_returns_entry() {
        let mut stack = NET_STACK.lock();
        let handle = stack.add_tcp_socket();
        let mut table = SocketTable::new();
        let entry = NetSocketEntry::new(handle, NetSocketType::Tcp, 2);
        let fd = table.insert(entry).unwrap();
        let removed = table.remove(fd).unwrap();
        assert_eq!(removed.sock_type, NetSocketType::Tcp);
        assert_eq!(removed.domain, 2);
        assert!(table.get(fd).is_none());
        drop(stack);
    }

    #[test]
    fn test_socket_table_remove_nonexistent_returns_none() {
        let mut table = SocketTable::new();
        assert!(table.remove(9999).is_none());
    }

    #[test]
    fn test_socket_table_overwrite_fd() {
        let mut stack = NET_STACK.lock();
        let h1 = stack.add_tcp_socket();
        let h2 = stack.add_udp_socket();
        let mut table = SocketTable::new();
        let fd = table
            .insert(NetSocketEntry::new(h1, NetSocketType::Tcp, 2))
            .unwrap();
        // Insert second entry (gets different FD since FDs are sequential)
        let fd2 = table
            .insert(NetSocketEntry::new(h2, NetSocketType::Udp, 2))
            .unwrap();
        assert_ne!(fd, fd2);
        assert_eq!(table.fds.len(), 2);
        drop(stack);
    }

    #[test]
    fn test_net_socket_entry_domain_af_inet() {
        let mut stack = NET_STACK.lock();
        let handle = stack.add_tcp_socket();
        let entry = NetSocketEntry::new(handle, NetSocketType::Tcp, 2);
        assert_eq!(entry.domain, 2); // AF_INET
        drop(stack);
    }

    #[test]
    fn test_net_socket_entry_domain_af_unix() {
        let mut stack = NET_STACK.lock();
        let handle = stack.add_udp_socket();
        let entry = NetSocketEntry::new(handle, NetSocketType::Udp, 1);
        assert_eq!(entry.domain, 1); // AF_UNIX
        drop(stack);
    }

    #[test]
    fn test_socket_table_fd_sequential() {
        let mut stack = NET_STACK.lock();
        let mut table = SocketTable::new();
        let h = stack.add_tcp_socket();
        let fd1 = table
            .insert(NetSocketEntry::new(h, NetSocketType::Tcp, 2))
            .unwrap();
        let h2 = stack.add_tcp_socket();
        let fd2 = table
            .insert(NetSocketEntry::new(h2, NetSocketType::Tcp, 2))
            .unwrap();
        assert_eq!(fd2, fd1 + 1, "FDs should be sequential");
        drop(stack);
    }

    #[test]
    fn test_listen_requires_bound_tcp_socket() {
        let fd = sys_socket(2, 1).unwrap();
        let err = sys_listen(fd, 8).unwrap_err();
        assert_eq!(err, EINVAL);
        assert!(sys_close(fd).is_ok());
    }

    #[test]
    fn test_bind_twice_returns_einval() {
        let fd = sys_socket(2, 1).unwrap();
        let addr = IpAddress::v4(127, 0, 0, 1);
        assert!(sys_bind(fd, addr, 9000).is_ok());
        let err = sys_bind(fd, addr, 9001).unwrap_err();
        assert_eq!(err, EINVAL);
        assert!(sys_close(fd).is_ok());
    }

    #[test]
    fn test_connect_rejects_listening_tcp_socket() {
        let fd = sys_socket(2, 1).unwrap();
        let addr = IpAddress::v4(127, 0, 0, 1);
        assert!(sys_bind(fd, addr, 9002).is_ok());
        assert!(sys_listen(fd, 4).is_ok());
        let err = sys_connect(fd, addr, 9003).unwrap_err();
        assert_eq!(err, EINVAL);
        assert!(sys_close(fd).is_ok());
    }
}
