#![no_std]

pub mod allocator;

use turnix_abi::syscall::Syscall;

pub fn print(message: &str) {
    syscall2(
        Syscall::Write as u64,
        message.as_ptr() as u64,
        message.len() as u64,
    );
}

pub fn println(message: &str) {
    print(message);
    print("\n");
}

pub fn read(fd: u64, buf: &mut [u8]) -> Option<u64> {
    let res = syscall3(
        Syscall::Read as u64,
        fd,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
    );
    if (res as i64) < 0 { None } else { Some(res) }
}

pub fn open(path: &str) -> Option<u64> {
    let res = syscall2(
        Syscall::Open as u64,
        path.as_ptr() as u64,
        path.len() as u64,
    );
    if (res as i64) < 0 { None } else { Some(res) }
}

pub fn close(fd: u64) {
    syscall1(Syscall::Close as u64, fd);
}

pub fn ls(buf: &mut [u8]) -> Option<u64> {
    let res = syscall2(
        Syscall::Ls as u64,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
    );
    if (res as i64) < 0 { None } else { Some(res) }
}

pub use turnix_abi::syscall::Stat;

pub fn stat(path: &str) -> Option<Stat> {
    let mut st = Stat::default();
    let res = syscall3(
        Syscall::Stat as u64,
        path.as_ptr() as u64,
        path.len() as u64,
        &mut st as *mut Stat as u64,
    );
    if (res as i64) < 0 { None } else { Some(st) }
}

pub fn exec(path: &str, argv: *const *const u8, envp: *const *const u8) -> ! {
    syscall4(
        Syscall::Exec as u64,
        path.as_ptr() as u64,
        path.len() as u64,
        argv as u64,
        envp as u64,
    );
    loop {
        core::hint::spin_loop();
    }
}

pub fn exit(code: i32) -> ! {
    syscall1(Syscall::Exit as u64, code as u64);
    loop {
        core::hint::spin_loop();
    }
}

pub fn wait(status: *mut i32) -> u64 {
    syscall1(Syscall::Wait as u64, status as u64)
}

pub fn waitpid(pid: i32, status: *mut i32, options: i32) -> u64 {
    syscall3(
        Syscall::Waitpid as u64,
        pid as u64,
        status as u64,
        options as u64,
    )
}

pub fn kill(pid: i32, sig: u8) -> u64 {
    syscall2(Syscall::Kill as u64, pid as u64, sig as u64)
}

pub fn sigaction(sig: u8, new: *const [u64; 3], old: *mut [u64; 3]) -> u64 {
    syscall3(
        Syscall::Sigaction as u64,
        sig as u64,
        new as u64,
        old as u64,
    )
}

pub fn sigprocmask(how: i32, new: *const u64, old: *mut u64) -> u64 {
    syscall3(
        Syscall::Sigprocmask as u64,
        how as u64,
        new as u64,
        old as u64,
    )
}

pub fn shutdown() -> ! {
    syscall0(Syscall::Shutdown as u64);
    loop {
        core::hint::spin_loop();
    }
}

pub fn read_shutdown_signal() -> u64 {
    syscall0(Syscall::ReadShutdownSignal as u64)
}

pub fn uptime() -> u64 {
    syscall0(Syscall::Uptime as u64)
}

pub fn getpid() -> u64 {
    syscall0(Syscall::GetPid as u64)
}

pub fn yielder() {
    syscall1(Syscall::Yielder as u64, 1);
}

pub fn fork() -> u64 {
    syscall0(Syscall::Fork as u64)
}

pub fn seek(fd: u64, offset: u64) -> bool {
    let res = syscall2(Syscall::Seek as u64, fd, offset);
    (res as i64) >= 0
}

pub fn write(fd: u64, buf: &[u8]) -> Option<u64> {
    let res = syscall3(
        Syscall::WriteFile as u64,
        fd,
        buf.as_ptr() as u64,
        buf.len() as u64,
    );
    if (res as i64) < 0 { None } else { Some(res) }
}

pub fn getuid() -> u64 {
    syscall0(Syscall::GetUid as u64)
}

pub fn getgid() -> u64 {
    syscall0(Syscall::GetGid as u64)
}

pub fn setuid(uid: u32) -> i64 {
    syscall1(Syscall::SetUid as u64, uid as u64) as i64
}

pub fn setgid(gid: u32) -> i64 {
    syscall1(Syscall::SetGid as u64, gid as u64) as i64
}

pub fn capset(header: &turnix_abi::syscall::CapHeader, data: &turnix_abi::syscall::CapData) -> i64 {
    syscall2(
        Syscall::Capset as u64,
        header as *const _ as u64,
        data as *const _ as u64,
    ) as i64
}

pub fn capget(
    header: &turnix_abi::syscall::CapHeader,
    data: &mut turnix_abi::syscall::CapData,
) -> i64 {
    syscall2(
        Syscall::Capget as u64,
        header as *const _ as u64,
        data as *mut _ as u64,
    ) as i64
}

pub fn mkdir(path: &str) -> bool {
    let res = syscall2(
        Syscall::Mkdir as u64,
        path.as_ptr() as u64,
        path.len() as u64,
    );
    (res as i64) >= 0
}

pub fn unlink(path: &str) -> bool {
    let res = syscall2(
        Syscall::Unlink as u64,
        path.as_ptr() as u64,
        path.len() as u64,
    );
    (res as i64) >= 0
}

pub fn dmesg(buf: &mut [u8]) -> Result<usize, i64> {
    let res = syscall2(
        Syscall::Dmesg as u64,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
    );
    if (res as i64) < 0 {
        Err(res as i64)
    } else {
        Ok(res as usize)
    }
}

/// Get the size of an extended attribute value from a file.
pub fn xattr_get(path: &str, name: &str) -> Result<usize, i64> {
    let res = syscall4(
        Syscall::XattrGet as u64,
        path.as_ptr() as u64,
        path.len() as u64,
        name.as_ptr() as u64,
        name.len() as u64,
    );
    if (res as i64) < 0 {
        Err(res as i64)
    } else {
        Ok(res as usize)
    }
}

/// Set an extended attribute on a file.
pub fn xattr_set(path: &str, name: &str, _value: &[u8]) -> Result<(), i64> {
    let res = syscall4(
        Syscall::XattrSet as u64,
        path.as_ptr() as u64,
        path.len() as u64,
        name.as_ptr() as u64,
        name.len() as u64,
    );
    if (res as i64) < 0 {
        Err(res as i64)
    } else {
        Ok(())
    }
}

fn syscall0(num: u64) -> u64 {
    let res: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            out("rcx") _,
            out("r11") _,
            lateout("rax") res,
        );
    }
    res
}

fn syscall1(num: u64, arg0: u64) -> u64 {
    let res: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            in("rdi") arg0,
            out("rcx") _,
            out("r11") _,
            lateout("rax") res,
        );
    }
    res
}

fn syscall2(num: u64, arg0: u64, arg1: u64) -> u64 {
    let res: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            in("rdi") arg0,
            in("rsi") arg1,
            out("rcx") _,
            out("r11") _,
            lateout("rax") res,
        );
    }
    res
}

fn syscall3(num: u64, arg0: u64, arg1: u64, arg2: u64) -> u64 {
    let res: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            in("rdi") arg0,
            in("rsi") arg1,
            in("rdx") arg2,
            out("rcx") _,
            out("r11") _,
            lateout("rax") res,
        );
    }
    res
}

// ── GPU / display syscall wrappers ───────────────────────────────────────

pub use turnix_abi::input::InputEvent;

/// Get the physical address of the scanout framebuffer.
pub fn mmap_framebuffer() -> Option<u64> {
    let res = syscall0(Syscall::MmapFramebuffer as u64);
    if (res as i64) < 0 { None } else { Some(res) }
}

/// Create a GBM buffer with the given dimensions and format.
pub fn gbm_create(width: u32, height: u32, format: u32) -> Option<u64> {
    let res = syscall3(
        Syscall::GbmCreate as u64,
        width as u64,
        height as u64,
        format as u64,
    );
    if (res as i64) < 0 { None } else { Some(res) }
}

/// Map a GBM buffer and return its physical address.
pub fn gbm_map(id: u64) -> Option<u64> {
    let res = syscall1(Syscall::GbmMap as u64, id);
    if (res as i64) < 0 { None } else { Some(res) }
}

/// Destroy a GBM buffer.
pub fn gbm_destroy(id: u64) {
    syscall1(Syscall::GbmDestroy as u64, id);
}

/// Perform a page flip to a GBM buffer.
pub fn drm_page_flip(gbm_id: u64, crtc_id: u32) -> i64 {
    syscall2(Syscall::DrmPageFlip as u64, gbm_id, crtc_id as u64) as i64
}

/// Read pending input events into the provided buffer.
/// Returns the number of events read.
pub fn input_read(events: &mut [InputEvent]) -> u64 {
    syscall2(
        Syscall::InputRead as u64,
        events.as_mut_ptr() as u64,
        events.len() as u64,
    )
}

// ── Socket syscall wrappers ─────────────────────────────────────────────

/// Create a socket. domain=1 for AF_UNIX, type=1 for SOCK_STREAM.
pub fn socket(domain: i32, sock_type: i32, protocol: i32) -> Option<u64> {
    let res = syscall3(
        Syscall::Socket as u64,
        domain as u64,
        sock_type as u64,
        protocol as u64,
    );
    if (res as i64) < 0 { None } else { Some(res) }
}

/// Bind a socket to a filesystem path.
pub fn bind(fd: u64, addr: *const u8, addr_len: usize) -> bool {
    let res = syscall3(Syscall::Bind as u64, fd, addr as u64, addr_len as u64);
    (res as i64) >= 0
}

/// Listen for incoming connections.
pub fn listen(fd: u64, backlog: usize) -> bool {
    let res = syscall2(Syscall::Listen as u64, fd, backlog as u64);
    (res as i64) >= 0
}

/// Accept a connection, returning the new fd.
pub fn accept(fd: u64) -> Option<u64> {
    let res = syscall1(Syscall::Accept as u64, fd);
    if (res as i64) < 0 { None } else { Some(res) }
}

/// Connect a socket to a server address (sockaddr with 2-byte family prefix).
pub fn connect(fd: u64, addr: *const u8, addr_len: usize) -> bool {
    let res = syscall3(Syscall::Connect as u64, fd, addr as u64, addr_len as u64);
    (res as i64) >= 0
}

// ── Network interface configuration syscall wrappers ────────────────────

/// Set the IP address, netmask, and gateway for a network interface.
pub fn net_set_addr(iface_id: u32, addr: [u8; 4], netmask: [u8; 4], gateway: [u8; 4]) -> i64 {
    syscall4(
        Syscall::NetSetAddr as u64,
        iface_id as u64,
        addr.as_ptr() as u64,
        netmask.as_ptr() as u64,
        gateway.as_ptr() as u64,
    ) as i64
}

/// Set the default gateway for interface 0.
pub fn net_set_route(gateway: [u8; 4]) -> i64 {
    syscall1(Syscall::NetSetRoute as u64, gateway.as_ptr() as u64) as i64
}

/// Query the current configuration of a network interface.
/// Returns `Some(NetQueryResp)` on success.
pub fn net_query(iface_id: u32) -> Option<turnix_abi::NetQueryResp> {
    let mut resp = turnix_abi::NetQueryResp {
        ip: [0; 4],
        netmask: [0; 4],
        gateway: [0; 4],
        mtu: 0,
        flags: 0,
    };
    let res = syscall2(
        Syscall::NetQuery as u64,
        iface_id as u64,
        &mut resp as *mut turnix_abi::NetQueryResp as u64,
    );
    if (res as i64) < 0 { None } else { Some(resp) }
}

fn syscall4(num: u64, arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    let res: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            in("rdi") arg0,
            in("rsi") arg1,
            in("rdx") arg2,
            in("r10") arg3,
            out("rcx") _,
            out("r11") _,
            lateout("rax") res,
        );
    }
    res
}
