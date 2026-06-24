use super::SyscallResult;
use crate::fs::vfs::UnixSocketState;
use crate::vfs::VFS;
use turnix_abi::syscall::SyscallArgs;

/// `socket(domain: i32, type: i32, protocol: i32) -> fd`
pub fn handle_socket(args: SyscallArgs) -> SyscallResult {
    let domain = args.arg0 as i32;
    let sock_type = args.arg1 as i32;
    let _protocol = args.arg2 as i32;

    match domain {
        1 => {
            // AF_UNIX — use existing VFS Unix socket
            if sock_type != 1 {
                return SyscallResult::Error(97); // EPROTONOSUPPORT
            }
            let mut vfs = VFS.lock();
            let fd = vfs.create_socket_fd();
            SyscallResult::Success(fd as u64)
        }
        2 | 10 => {
            // AF_INET or AF_INET6 — use smoltcp
            match crate::net::socket::sys_socket(domain, sock_type) {
                Ok(fd) => SyscallResult::Success(fd as u64),
                Err(e) => SyscallResult::Error(e),
            }
        }
        _ => SyscallResult::Error(97), // EAFNOSUPPORT
    }
}

/// `bind(sockfd: i32, addr: *const u8, addrlen: usize) -> 0`
pub fn handle_bind(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let addr_ptr = args.arg1 as *const u8;
    let addr_len = args.arg2 as usize;

    if addr_ptr.is_null() || addr_len < 2 {
        return SyscallResult::Error(14); // EFAULT
    }

    // Read the sa_family (first 2 bytes)
    let family = unsafe { core::ptr::read_unaligned(addr_ptr as *const u16) };

    match family {
        1 => {
            // AF_UNIX — sockaddr_un with sun_path as path
            if addr_len < 3 {
                return SyscallResult::Error(14);
            }
            let path_slice = unsafe { core::slice::from_raw_parts(addr_ptr.add(2), addr_len - 2) };
            // Trim trailing nulls
            let path_len = path_slice
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(path_slice.len());
            let path = core::str::from_utf8(&path_slice[..path_len]).unwrap_or("");
            if path.is_empty() {
                return SyscallResult::Error(14);
            }

            let sock = {
                let vfs = VFS.lock();
                vfs.get_unix_socket(fd)
            };
            let sock = match sock {
                Some(s) => s,
                None => return SyscallResult::Error(9), // EBADF
            };

            match UnixSocketState::bind(&sock, path) {
                Ok(_) => SyscallResult::Success(0),
                Err(_) => SyscallResult::Error(48), // EADDRINUSE
            }
        }
        2 | 10 => {
            // AF_INET or AF_INET6 — parse sockaddr_in
            // Check CAP_NET_ADMIN for network bind
            if let Some(current) = crate::task::scheduler::get_current_process()
                && !current
                    .inner
                    .lock()
                    .sec_ctx
                    .has_capability(crate::security::capabilities::Capability::NetAdmin)
            {
                return SyscallResult::Error(1); // EPERM
            }

            if addr_len < 8 {
                return SyscallResult::Error(14);
            }

            // sockaddr_in layout: family(2) + port(2) + addr(4) + zero(8)
            let raw_port = unsafe { core::ptr::read_unaligned(addr_ptr.add(2) as *const u16) };
            let raw_addr = unsafe { core::ptr::read_unaligned(addr_ptr.add(4) as *const u32) };

            let port = u16::from_be(raw_port);
            let ip = smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::from_bytes(
                &raw_addr.to_be_bytes(),
            ));

            match crate::net::socket::sys_bind(fd, ip, port) {
                Ok(()) => SyscallResult::Success(0),
                Err(e) => SyscallResult::Error(e),
            }
        }
        _ => SyscallResult::Error(97), // EAFNOSUPPORT
    }
}

/// `listen(sockfd: i32, backlog: i32) -> 0`
pub fn handle_listen(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let backlog = args.arg1 as usize;

    // Try AF_INET first
    if let Ok(()) = crate::net::socket::sys_listen(fd, backlog) {
        return SyscallResult::Success(0);
    }

    // Fall back to AF_UNIX
    let sock = {
        let vfs = VFS.lock();
        vfs.get_unix_socket(fd)
    };
    let sock = match sock {
        Some(s) => s,
        None => return SyscallResult::Error(9), // EBADF
    };

    match UnixSocketState::listen(&sock, backlog) {
        Ok(_) => SyscallResult::Success(0),
        Err(_) => SyscallResult::Error(88), // ENOTSOCK / EINVAL
    }
}

/// `accept(sockfd: i32) -> new_fd`
pub fn handle_accept(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;

    // Try AF_INET first
    if let Ok(new_fd) = crate::net::socket::sys_accept(fd) {
        return SyscallResult::Success(new_fd as u64);
    }

    // Fall back to AF_UNIX
    let sock = {
        let vfs = VFS.lock();
        vfs.get_unix_socket(fd)
    };
    let sock = match sock {
        Some(s) => s,
        None => return SyscallResult::Error(9), // EBADF
    };

    match UnixSocketState::accept(&sock) {
        Some(end) => {
            let mut vfs = VFS.lock();
            let idx = vfs.create_connected_socket_fd(end);
            SyscallResult::Success(idx as u64)
        }
        None => SyscallResult::Error(11), // EAGAIN
    }
}

/// `connect(sockfd: i32, addr: *const u8, addrlen: usize) -> 0`
pub fn handle_connect(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let addr_ptr = args.arg1 as *const u8;
    let addr_len = args.arg2 as usize;

    if addr_ptr.is_null() || addr_len < 2 {
        return SyscallResult::Error(14); // EFAULT
    }

    let family = unsafe { core::ptr::read_unaligned(addr_ptr as *const u16) };

    match family {
        1 => {
            // AF_UNIX — sockaddr_un path
            if addr_len < 3 {
                return SyscallResult::Error(14);
            }
            let path_slice = unsafe { core::slice::from_raw_parts(addr_ptr.add(2), addr_len - 2) };
            let path_len = path_slice
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(path_slice.len());
            let path = core::str::from_utf8(&path_slice[..path_len]).unwrap_or("");
            if path.is_empty() {
                return SyscallResult::Error(14);
            }

            let sock = {
                let vfs = VFS.lock();
                vfs.get_unix_socket(fd)
            };
            let sock = match sock {
                Some(s) => s,
                None => return SyscallResult::Error(9), // EBADF
            };

            // LSM net_connect hook
            let ctx = crate::security::current_context();
            if crate::security::lsm::check_net_connect(path, ctx.uid, ctx.gid).is_err() {
                return SyscallResult::Error(1); // EPERM
            }

            match UnixSocketState::connect(&sock, path) {
                Ok(_) => SyscallResult::Success(0),
                Err(_) => SyscallResult::Error(2), // ENOENT
            }
        }
        2 | 10 => {
            // AF_INET or AF_INET6
            if addr_len < 8 {
                return SyscallResult::Error(14);
            }

            let raw_port = unsafe { core::ptr::read_unaligned(addr_ptr.add(2) as *const u16) };
            let raw_addr = unsafe { core::ptr::read_unaligned(addr_ptr.add(4) as *const u32) };

            let port = u16::from_be(raw_port);
            let ip = smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::from_bytes(
                &raw_addr.to_be_bytes(),
            ));

            match crate::net::socket::sys_connect(fd, ip, port) {
                Ok(()) => SyscallResult::Success(0),
                Err(e) => SyscallResult::Error(e),
            }
        }
        _ => SyscallResult::Error(97), // EAFNOSUPPORT
    }
}

/// `NetSetAddr(iface_id, addr_ptr, netmask_ptr, gateway_ptr) -> 0`
///
/// Sets the IP address, netmask, and gateway for a network interface.
/// - arg0: interface ID (0..7)
/// - arg1: pointer to 4-byte IPv4 address
/// - arg2: pointer to 4-byte netmask
/// - arg3: pointer to 4-byte gateway
pub fn handle_net_set_addr(args: SyscallArgs) -> SyscallResult {
    // Check CAP_NET_ADMIN
    if let Some(current) = crate::task::scheduler::get_current_process()
        && !current
            .inner
            .lock()
            .sec_ctx
            .has_capability(crate::security::capabilities::Capability::NetAdmin)
    {
        return SyscallResult::Error(1); // EPERM
    }

    let iface_id = args.arg0 as u32;
    let addr_ptr = args.arg1 as *const [u8; 4];
    let netmask_ptr = args.arg2 as *const [u8; 4];
    let gateway_ptr = args.arg3 as *const [u8; 4];

    if iface_id >= 8 || addr_ptr.is_null() || netmask_ptr.is_null() || gateway_ptr.is_null() {
        return SyscallResult::Error(22); // EINVAL
    }

    let addr = unsafe { core::ptr::read_unaligned(addr_ptr) };
    let netmask = unsafe { core::ptr::read_unaligned(netmask_ptr) };
    let gateway = unsafe { core::ptr::read_unaligned(gateway_ptr) };

    // Apply to smoltcp interface as well
    {
        let mut stack = crate::net::smoltcp_iface::NET_STACK.lock();
        let prefix_len = netmask
            .iter()
            .fold(0u8, |acc, b| acc + b.count_ones() as u8);
        let ip_cidr = smoltcp::wire::IpCidr::new(
            smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::from_bytes(&addr)),
            prefix_len,
        );
        stack.interface.update_ip_addrs(|addrs| {
            addrs.clear();
            let _ = addrs.push(ip_cidr);
        });

        // Add default route via gateway
        if gateway != [0, 0, 0, 0] {
            let gw = smoltcp::wire::Ipv4Address::from_bytes(&gateway);
            let _ = stack.interface.routes_mut().add_default_ipv4_route(gw);
        }
    }

    {
        let mut configs = crate::net::NET_CONFIGS.lock();
        let cfg = &mut configs[iface_id as usize];
        cfg.ip = addr;
        cfg.netmask = netmask;
        cfg.gateway = gateway;
        cfg.up = true;
    }

    crate::serial::println!(
        "[NET] set_addr iface={} ip={}.{}.{}.{} nm={}.{}.{}.{} gw={}.{}.{}.{}",
        iface_id,
        addr[0],
        addr[1],
        addr[2],
        addr[3],
        netmask[0],
        netmask[1],
        netmask[2],
        netmask[3],
        gateway[0],
        gateway[1],
        gateway[2],
        gateway[3],
    );

    SyscallResult::Success(0)
}

/// `NetSetRoute(gateway_ptr) -> 0`
///
/// Sets the default gateway on interface 0.
/// - arg0: pointer to 4-byte gateway address
pub fn handle_net_set_route(args: SyscallArgs) -> SyscallResult {
    // Check CAP_NET_ADMIN
    if let Some(current) = crate::task::scheduler::get_current_process()
        && !current
            .inner
            .lock()
            .sec_ctx
            .has_capability(crate::security::capabilities::Capability::NetAdmin)
    {
        return SyscallResult::Error(1); // EPERM
    }

    let gateway_ptr = args.arg0 as *const [u8; 4];
    if gateway_ptr.is_null() {
        return SyscallResult::Error(22); // EINVAL
    }

    let gateway = unsafe { core::ptr::read_unaligned(gateway_ptr) };

    {
        let mut configs = crate::net::NET_CONFIGS.lock();
        configs[0].gateway = gateway;
    }

    crate::serial::println!(
        "[NET] set_route gw={}.{}.{}.{}",
        gateway[0],
        gateway[1],
        gateway[2],
        gateway[3],
    );

    SyscallResult::Success(0)
}

/// `NetQuery(iface_id, resp_ptr) -> 0`
///
/// Queries the current configuration of a network interface.
/// - arg0: interface ID (0..7)
/// - arg1: pointer to NetQueryResp (16 bytes)
pub fn handle_net_query(args: SyscallArgs) -> SyscallResult {
    let iface_id = args.arg0 as u32;
    let resp_ptr = args.arg1 as *mut turnix_abi::NetQueryResp;

    if iface_id >= 8 || resp_ptr.is_null() {
        return SyscallResult::Error(22); // EINVAL
    }

    let configs = crate::net::NET_CONFIGS.lock();
    let cfg = &configs[iface_id as usize];

    let resp = turnix_abi::NetQueryResp {
        ip: cfg.ip,
        netmask: cfg.netmask,
        gateway: cfg.gateway,
        mtu: cfg.mtu,
        flags: if cfg.up { 1 } else { 0 },
    };

    unsafe { core::ptr::write_unaligned(resp_ptr, resp) };

    SyscallResult::Success(0)
}

/// `shutdown() -> !`
/// Powers off the machine.  Never returns.
pub fn handle_shutdown(_args: SyscallArgs) -> SyscallResult {
    crate::serial::println!("[syscall] shutdown() called by init");
    // QEMU/ACPI poweroff: try several common ports.
    unsafe {
        // QEMU
        core::arch::asm!("outw %ax, %dx", in("ax") 0x2000u16, in("dx") 0x604u16, options(att_syntax));
        // Bochs/older QEMU fallback
        core::arch::asm!("outw %ax, %dx", in("ax") 0x2000u16, in("dx") 0xB004u16, options(att_syntax));
    }
    loop {
        x86_64::instructions::hlt();
    }
}

/// `read_shutdown_signal() -> u64`
/// Returns 1 if the ACPI power-button shutdown signal has been received,
/// 0 otherwise.  Consumes the signal (clears it after reading).
pub fn handle_read_shutdown_signal(_args: SyscallArgs) -> SyscallResult {
    let pending = crate::acpi::take_init_shutdown_signal();
    SyscallResult::Success(if pending { 1 } else { 0 })
}

/// send(fd, buf_ptr, buf_len, flags) -> bytes_sent
pub fn handle_send(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let buf_ptr = args.arg1 as *const u8;
    let buf_len = args.arg2 as usize;
    let _flags = args.arg3 as i32;

    if buf_ptr.is_null() || buf_len == 0 {
        return SyscallResult::Error(22); // EINVAL
    }

    let buf = unsafe { core::slice::from_raw_parts(buf_ptr, buf_len) };

    match crate::net::socket::sys_send(fd, buf) {
        Ok(n) => SyscallResult::Success(n as u64),
        Err(e) => SyscallResult::Error(e),
    }
}

/// recv(fd, buf_ptr, buf_len, flags) -> bytes_received
pub fn handle_recv(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let buf_ptr = args.arg1 as *mut u8;
    let buf_len = args.arg2 as usize;
    let _flags = args.arg3 as i32;

    if buf_ptr.is_null() || buf_len == 0 {
        return SyscallResult::Error(22); // EINVAL
    }

    let mut buf = alloc::vec![0u8; buf_len];

    match crate::net::socket::sys_recv(fd, &mut buf) {
        Ok(n) => {
            unsafe {
                core::ptr::copy_nonoverlapping(buf.as_ptr(), buf_ptr, n);
            }
            SyscallResult::Success(n as u64)
        }
        Err(e) => SyscallResult::Error(e),
    }
}
