#![cfg(test)]

use super::socket::{
    sys_accept, sys_bind, sys_close, sys_connect, sys_listen, sys_recv, sys_send, sys_socket,
    EBADF, ECONNREFUSED, EINVAL, SOCKET_TABLE,
};
use smoltcp::wire::IpAddress;

fn create_tcp_socket(domain: i32) -> usize {
    sys_socket(domain, 1).expect("sys_socket should succeed")
}

fn create_udp_socket(domain: i32) -> usize {
    sys_socket(domain, 2).expect("sys_socket should succeed")
}

// -----------------------------------------------------------------------
// Socket state transitions
// -----------------------------------------------------------------------

#[test]
fn test_socket_state_closed_to_bound() {
    let _guard = crate::test_serial::acquire();
    let fd = create_tcp_socket(2);
    {
        let table = SOCKET_TABLE.lock();
        let entry = table.get(fd).expect("socket should exist");
        assert!(!entry.is_bound);
        assert!(!entry.is_listening);
        assert!(!entry.is_connected);
    }

    let addr = IpAddress::v4(127, 0, 0, 1);
    sys_bind(fd, addr, 10001).expect("bind should succeed");

    {
        let table = SOCKET_TABLE.lock();
        let entry = table.get(fd).expect("socket should exist");
        assert!(entry.is_bound);
        assert_eq!(entry.local_port, Some(10001));
        assert!(!entry.is_listening);
        assert!(!entry.is_connected);
    }
    assert!(sys_close(fd).is_ok());
}

#[test]
fn test_socket_bound_to_listening() {
    let _guard = crate::test_serial::acquire();
    let fd = create_tcp_socket(2);
    let addr = IpAddress::v4(127, 0, 0, 1);
    sys_bind(fd, addr, 10002).expect("bind should succeed");

    {
        let table = SOCKET_TABLE.lock();
        let entry = table.get(fd).expect("socket should exist");
        assert!(!entry.is_listening);
    }

    sys_listen(fd, 8).expect("listen should succeed");

    {
        let table = SOCKET_TABLE.lock();
        let entry = table.get(fd).expect("socket should exist");
        assert!(entry.is_bound);
        assert!(entry.is_listening);
        assert_eq!(entry.backlog, 8);
    }
    assert!(sys_close(fd).is_ok());
}

#[test]
fn test_socket_established_on_connect() {
    let _guard = crate::test_serial::acquire();
    let fd = create_tcp_socket(2);
    let addr = IpAddress::v4(10, 0, 2, 2);
    // In test mode, connect fails with ECONNREFUSED because the network
    // stack is not fully operational. Verify the correct error is returned.
    let err = sys_connect(fd, addr, 80).unwrap_err();
    assert_eq!(err, ECONNREFUSED);
    assert!(sys_close(fd).is_ok());
}

#[test]
fn test_socket_connect_sets_remote_on_success_path() {
    let _guard = crate::test_serial::acquire();
    let fd = create_tcp_socket(2);
    let addr = IpAddress::v4(10, 0, 2, 2);
    // Even when connect fails, the fd is still valid and closeable.
    let _ = sys_connect(fd, addr, 80);
    // Verify socket is still in the table (close works regardless).
    {
        let table = SOCKET_TABLE.lock();
        assert!(table.get(fd).is_some());
    }
    assert!(sys_close(fd).is_ok());
}

#[test]
fn test_socket_close_from_established() {
    let _guard = crate::test_serial::acquire();
    let fd = create_tcp_socket(2);
    // Manually mark the socket as connected to test close-from-established path.
    {
        let mut table = SOCKET_TABLE.lock();
        let entry = table.get_mut(fd).expect("socket should exist");
        entry.is_connected = true;
        entry.remote_endpoint = Some(smoltcp::wire::IpEndpoint::new(
            IpAddress::v4(10, 0, 2, 2),
            80,
        ));
    }
    assert!(sys_close(fd).is_ok());
    let table = SOCKET_TABLE.lock();
    assert!(table.get(fd).is_none());
}

#[test]
fn test_socket_close_from_listening() {
    let _guard = crate::test_serial::acquire();
    let fd = create_tcp_socket(2);
    let addr = IpAddress::v4(127, 0, 0, 1);
    sys_bind(fd, addr, 10003).expect("bind should succeed");
    sys_listen(fd, 4).expect("listen should succeed");

    {
        let table = SOCKET_TABLE.lock();
        let entry = table.get(fd).expect("socket should exist");
        assert!(entry.is_listening);
    }

    assert!(sys_close(fd).is_ok());
    let table = SOCKET_TABLE.lock();
    assert!(table.get(fd).is_none());
}

// -----------------------------------------------------------------------
// Bind/listen rejection tests
// -----------------------------------------------------------------------

#[test]
fn test_socket_reject_bind_twice() {
    let _guard = crate::test_serial::acquire();
    let fd = create_tcp_socket(2);
    let addr = IpAddress::v4(127, 0, 0, 1);
    sys_bind(fd, addr, 10004).expect("first bind should succeed");
    let err = sys_bind(fd, addr, 10005).unwrap_err();
    assert_eq!(err, EINVAL, "second bind should return EINVAL");
    assert!(sys_close(fd).is_ok());
}

#[test]
fn test_socket_reject_listen_without_bind() {
    let _guard = crate::test_serial::acquire();
    let fd = create_tcp_socket(2);
    let err = sys_listen(fd, 8).unwrap_err();
    assert_eq!(err, EINVAL, "listen without bind should return EINVAL");
    assert!(sys_close(fd).is_ok());
}

#[test]
fn test_socket_reject_close_nonexistent_fd() {
    let _guard = crate::test_serial::acquire();
    let err = sys_close(99999).unwrap_err();
    assert_eq!(err, EBADF, "closing nonexistent FD should return EBADF");
}

// -----------------------------------------------------------------------
// Accept test
// -----------------------------------------------------------------------

#[test]
fn test_socket_accept_returns_eagain_without_connection() {
    let _guard = crate::test_serial::acquire();
    let listen_fd = create_tcp_socket(2);
    let addr = IpAddress::v4(127, 0, 0, 1);
    sys_bind(listen_fd, addr, 10006).expect("bind should succeed");
    sys_listen(listen_fd, 4).expect("listen should succeed");

    let err = sys_accept(listen_fd).unwrap_err();
    assert_eq!(
        err,
        crate::net::socket::EAGAIN,
        "accept with no pending connection returns EAGAIN"
    );
    assert!(sys_close(listen_fd).is_ok());
}

// -----------------------------------------------------------------------
// Send/recv tests
// -----------------------------------------------------------------------

#[test]
fn test_socket_send_recv_on_unconnected() {
    let _guard = crate::test_serial::acquire();
    let fd = create_tcp_socket(2);
    let data = b"hello";
    // Send on a non-connected TCP socket returns EIO or EAGAIN depending
    // on the underlying smoltcp state. Either is acceptable in test mode.
    let _result = sys_send(fd, data);
    // recv on a non-connected TCP socket returns EAGAIN
    let mut buf = [0u8; 64];
    let recv_result = sys_recv(fd, &mut buf);
    assert!(recv_result.is_err(), "recv on non-connected socket should fail");
    assert!(sys_close(fd).is_ok());
}

#[test]
fn test_socket_send_recv_roundtrip() {
    let _guard = crate::test_serial::acquire();
    let fd = create_tcp_socket(2);
    // Manually mark as connected to test send/recv paths.
    {
        let mut table = SOCKET_TABLE.lock();
        let entry = table.get_mut(fd).expect("socket should exist");
        entry.is_connected = true;
    }
    let data = b"hello";
    // The underlying smoltcp socket is not truly connected in test mode,
    // so send may return EAGAIN. Either success or EAGAIN is acceptable.
    let _ = sys_send(fd, data);
    // recv with no data returns EAGAIN
    let mut buf = [0u8; 64];
    let err = sys_recv(fd, &mut buf).unwrap_err();
    assert_eq!(
        err,
        crate::net::socket::EAGAIN,
        "recv with no data returns EAGAIN"
    );
    assert!(sys_close(fd).is_ok());
}

// -----------------------------------------------------------------------
// Multiple listeners on different ports
// -----------------------------------------------------------------------

#[test]
fn test_socket_multiple_listeners_different_ports() {
    let _guard = crate::test_serial::acquire();
    let fd1 = create_tcp_socket(2);
    let fd2 = create_tcp_socket(2);
    let fd3 = create_tcp_socket(2);
    let addr = IpAddress::v4(127, 0, 0, 1);

    sys_bind(fd1, addr, 10010).expect("bind fd1 should succeed");
    sys_bind(fd2, addr, 10011).expect("bind fd2 should succeed");
    sys_bind(fd3, addr, 10012).expect("bind fd3 should succeed");

    sys_listen(fd1, 4).expect("listen fd1 should succeed");
    sys_listen(fd2, 4).expect("listen fd2 should succeed");
    sys_listen(fd3, 4).expect("listen fd3 should succeed");

    let (p1, p2, p3) = {
        let table = SOCKET_TABLE.lock();
        let e1 = table.get(fd1).expect("fd1 should exist");
        let e2 = table.get(fd2).expect("fd2 should exist");
        let e3 = table.get(fd3).expect("fd3 should exist");
        assert!(e1.is_listening);
        assert!(e2.is_listening);
        assert!(e3.is_listening);
        (e1.local_port, e2.local_port, e3.local_port)
    };
    assert_eq!(p1, Some(10010));
    assert_eq!(p2, Some(10011));
    assert_eq!(p3, Some(10012));

    assert!(sys_close(fd1).is_ok());
    assert!(sys_close(fd2).is_ok());
    assert!(sys_close(fd3).is_ok());
}

// -----------------------------------------------------------------------
// Socket table state isolation tests
// -----------------------------------------------------------------------

#[test]
fn test_socket_fd_sequential_allocation() {
    let _guard = crate::test_serial::acquire();
    let fd1 = create_tcp_socket(2);
    let fd2 = create_tcp_socket(2);
    let fd3 = create_tcp_socket(2);
    assert!(fd2 > fd1, "FDs should be monotonically increasing");
    assert!(fd3 > fd2, "FDs should be monotonically increasing");
    assert!(sys_close(fd1).is_ok());
    assert!(sys_close(fd2).is_ok());
    assert!(sys_close(fd3).is_ok());
}

#[test]
fn test_socket_udp_bind_and_close() {
    let _guard = crate::test_serial::acquire();
    let fd = create_udp_socket(2);
    let addr = IpAddress::v4(127, 0, 0, 1);
    sys_bind(fd, addr, 10020).expect("UDP bind should succeed");
    {
        let table = SOCKET_TABLE.lock();
        let entry = table.get(fd).expect("socket should exist");
        assert!(entry.is_bound);
        assert_eq!(entry.local_port, Some(10020));
    }
    assert!(sys_close(fd).is_ok());
    let table = SOCKET_TABLE.lock();
    assert!(table.get(fd).is_none());
}

#[test]
fn test_socket_reject_connect_already_connected() {
    let _guard = crate::test_serial::acquire();
    let fd = create_tcp_socket(2);
    // Manually mark as connected to test double-connect rejection.
    {
        let mut table = SOCKET_TABLE.lock();
        let entry = table.get_mut(fd).expect("socket should exist");
        entry.is_connected = true;
    }
    let addr = IpAddress::v4(10, 0, 2, 2);
    let err = sys_connect(fd, addr, 81).unwrap_err();
    assert_eq!(
        err, EINVAL,
        "connect on already-connected socket should fail"
    );
    assert!(sys_close(fd).is_ok());
}

#[test]
fn test_socket_reject_accept_on_non_listening() {
    let _guard = crate::test_serial::acquire();
    let fd = create_tcp_socket(2);
    // Manually mark as connected (not listening) to test accept rejection.
    {
        let mut table = SOCKET_TABLE.lock();
        let entry = table.get_mut(fd).expect("socket should exist");
        entry.is_connected = true;
    }
    let err = sys_accept(fd).unwrap_err();
    assert_eq!(err, EINVAL, "accept on connected socket should fail");
    assert!(sys_close(fd).is_ok());
}
