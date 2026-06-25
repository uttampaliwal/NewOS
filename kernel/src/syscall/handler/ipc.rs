use super::{SyscallResult, helper_alloc_fd};
use crate::vfs::VFS;
use alloc::sync::Arc;
use turnix_abi::syscall::SyscallArgs;

// ---------------------------------------------------------------------------
// POSIX Shared Memory syscalls
// ---------------------------------------------------------------------------

pub fn handle_shm_open(args: SyscallArgs) -> SyscallResult {
    let name_ptr = args.arg0 as *const u8;
    let name_len = args.arg1 as usize;
    let flags = args.arg2 as i32;
    let _mode = args.arg3;

    if name_ptr.is_null() || name_len == 0 {
        return SyscallResult::Error(22); // EINVAL
    }

    // Safety: name_ptr is validated non-null and name_len > 0 above; caller guarantees the pointer
    // references a valid readable buffer of at least name_len bytes.
    let name_slice = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let name = match core::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return SyscallResult::Error(22),
    };

    // Validate name doesn't contain slashes (POSIX requirement)
    if name.contains('/') {
        return SyscallResult::Error(22);
    }

    // Build the /dev/shm/ path
    let mut path = alloc::string::String::from("/dev/shm/");
    path.push_str(name);

    // Map flags to OpenFlags
    let mut open_flags = crate::vfs::OpenFlags::empty();
    open_flags |= crate::vfs::OpenFlags::RDWR;
    if flags & 0x40 != 0 {
        // O_CREAT
        open_flags |= crate::vfs::OpenFlags::CREAT;
    }
    if flags & 0x200 != 0 {
        // O_EXCL
        open_flags |= crate::vfs::OpenFlags::EXCL;
    }
    if flags & 0x400 != 0 {
        // O_TRUNC
        open_flags |= crate::vfs::OpenFlags::TRUNC;
    }

    let mut vfs = VFS.lock();
    // Ensure /dev/shm/ directory exists.
    vfs.mkdir_path("/dev/shm");

    match vfs.open_with_creds(&path, open_flags, 0, 0) {
        Ok(fd) => SyscallResult::Success(fd as u64),
        Err(_) => SyscallResult::Error(2), // ENOENT
    }
}

pub fn handle_shm_unlink(args: SyscallArgs) -> SyscallResult {
    let name_ptr = args.arg0 as *const u8;
    let name_len = args.arg1 as usize;

    if name_ptr.is_null() || name_len == 0 {
        return SyscallResult::Error(22);
    }

    // Safety: name_ptr is validated non-null and name_len > 0 above; caller guarantees the pointer
    // references a valid readable buffer of at least name_len bytes.
    let name_slice = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let name = match core::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return SyscallResult::Error(22),
    };

    if name.contains('/') {
        return SyscallResult::Error(22);
    }

    let mut path = alloc::string::String::from("/dev/shm/");
    path.push_str(name);

    let mut vfs = VFS.lock();
    if vfs.unlink(&path) {
        SyscallResult::Success(0)
    } else {
        SyscallResult::Error(2)
    }
}

// ---------------------------------------------------------------------------
// POSIX Message Queue syscalls
// ---------------------------------------------------------------------------

pub fn handle_mq_open(args: SyscallArgs) -> SyscallResult {
    let name_ptr = args.arg0 as *const u8;
    let name_len = args.arg1 as usize;
    let flags = args.arg2 as i32;
    let mode = args.arg3 as u32;
    let max_msgs = args.arg4 as usize;
    let max_msg_size = args.arg5 as usize;

    if name_ptr.is_null() || name_len == 0 {
        return SyscallResult::Error(22);
    }

    // Safety: name_ptr is validated non-null and name_len > 0 above; caller guarantees the pointer
    // references a valid readable buffer of at least name_len bytes.
    let name_slice = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let name = match core::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return SyscallResult::Error(22),
    };

    if name.contains('/') {
        return SyscallResult::Error(22);
    }

    let max_msgs = if max_msgs == 0 {
        crate::ipc::mqueue::MQ_MAX_MSG
    } else {
        max_msgs
    };
    let max_msg_size = if max_msg_size == 0 {
        crate::ipc::mqueue::MQ_MSG_SIZE
    } else {
        max_msg_size
    };

    let mq = match crate::ipc::mqueue::mq_open(name, flags, mode, max_msgs, max_msg_size) {
        Ok(q) => q,
        Err(e) => return SyscallResult::Error(e as i64),
    };

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let mut inner = process.inner.lock();
    let fd = inner
        .fd_table
        .iter()
        .position(|s| s.is_none())
        .unwrap_or(inner.fd_table.len());
    if fd >= inner.fd_table.len() {
        inner.fd_table.resize_with(fd + 1, || None);
    }

    inner.fd_table[fd] = Some(crate::vfs::FileDescriptor {
        inode: crate::vfs::InodeId(0),
        backend: Arc::new(crate::fs::tmpfs::TmpfsBackend::new()),
        offset: core::sync::atomic::AtomicU64::new(0),
        flags: crate::vfs::OpenFlags::RDWR,
        kind: crate::vfs::FdKind::MessageQueue(mq),
        name: alloc::string::String::from(name),
    });

    SyscallResult::Success(fd as u64)
}

pub fn handle_mq_close(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let mut inner = process.inner.lock();
    match inner.fd_table.get_mut(fd) {
        Some(Some(_)) => {
            inner.fd_table[fd] = None;
            SyscallResult::Success(0)
        }
        _ => SyscallResult::Error(9), // EBADF
    }
}

pub fn handle_mq_unlink(args: SyscallArgs) -> SyscallResult {
    let name_ptr = args.arg0 as *const u8;
    let name_len = args.arg1 as usize;

    if name_ptr.is_null() || name_len == 0 {
        return SyscallResult::Error(22);
    }

    // Safety: name_ptr is validated non-null and name_len > 0 above; caller guarantees the pointer
    // references a valid readable buffer of at least name_len bytes.
    let name_slice = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let name = match core::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return SyscallResult::Error(22),
    };

    match crate::ipc::mqueue::mq_unlink(name) {
        Ok(()) => SyscallResult::Success(0),
        Err(e) => SyscallResult::Error(e as i64),
    }
}

pub fn handle_mq_send(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let msg_ptr = args.arg1 as *const u8;
    let msg_len = args.arg2 as usize;
    let prio = args.arg3 as u32;

    if msg_ptr.is_null() {
        return SyscallResult::Error(14); // EFAULT
    }

    // Safety: msg_ptr is validated non-null above; caller guarantees the pointer references
    // a valid readable buffer of at least msg_len bytes.
    let msg_slice = unsafe { core::slice::from_raw_parts(msg_ptr, msg_len) };

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let inner = process.inner.lock();
    let mq = match inner.fd_table.get(fd).and_then(|e| e.as_ref()) {
        Some(entry) => match &entry.kind {
            crate::vfs::FdKind::MessageQueue(q) => Arc::clone(q),
            _ => return SyscallResult::Error(9),
        },
        _ => return SyscallResult::Error(9),
    };
    drop(inner);

    match mq.send(msg_slice, prio) {
        Ok(()) => SyscallResult::Success(0),
        Err(e) => SyscallResult::Error(e as i64),
    }
}

pub fn handle_mq_receive(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let buf_ptr = args.arg1 as *mut u8;
    let buf_len = args.arg2 as usize;

    if buf_ptr.is_null() {
        return SyscallResult::Error(14); // EFAULT
    }

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let inner = process.inner.lock();
    let mq = match inner.fd_table.get(fd).and_then(|e| e.as_ref()) {
        Some(entry) => match &entry.kind {
            crate::vfs::FdKind::MessageQueue(q) => Arc::clone(q),
            _ => return SyscallResult::Error(9),
        },
        _ => return SyscallResult::Error(9),
    };
    drop(inner);

    let mut buf = alloc::vec![0u8; buf_len];
    match mq.receive(&mut buf) {
        Ok((n, _prio)) => {
            // Safety: buf_ptr is validated non-null above; n bytes received is <= buf_len
            // (the size of our local buffer), so the copy is within bounds.
            unsafe {
                core::ptr::copy_nonoverlapping(buf.as_ptr(), buf_ptr, n);
            }
            SyscallResult::Success(n as u64)
        }
        Err(e) => SyscallResult::Error(e as i64),
    }
}

// ---------------------------------------------------------------------------
// Epoll syscalls
// ---------------------------------------------------------------------------

pub fn handle_epoll_create(_args: SyscallArgs) -> SyscallResult {
    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let epfd = match crate::ipc::epoll::epoll_create() {
        Ok(id) => id,
        Err(e) => return SyscallResult::Error(e as i64),
    };

    let instance = match crate::ipc::epoll::epoll_get(epfd) {
        Some(inst) => inst,
        None => return SyscallResult::Error(12),
    };

    let mut inner = process.inner.lock();
    let fd = inner
        .fd_table
        .iter()
        .position(|s| s.is_none())
        .unwrap_or(inner.fd_table.len());
    if fd >= inner.fd_table.len() {
        inner.fd_table.resize_with(fd + 1, || None);
    }

    inner.fd_table[fd] = Some(crate::vfs::FileDescriptor {
        inode: crate::vfs::InodeId(0),
        backend: Arc::new(crate::fs::tmpfs::TmpfsBackend::new()),
        offset: core::sync::atomic::AtomicU64::new(0),
        flags: crate::vfs::OpenFlags::RDWR,
        kind: crate::vfs::FdKind::Epoll(instance),
        name: alloc::string::String::from("epoll"),
    });

    SyscallResult::Success(fd as u64)
}

pub fn handle_epoll_ctl(args: SyscallArgs) -> SyscallResult {
    let epfd = args.arg0 as usize;
    let op = args.arg1 as u32;
    let fd = args.arg2 as usize;
    let events = args.arg3 as u32;
    let data = args.arg4;

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let inner = process.inner.lock();
    let instance = match inner.fd_table.get(epfd).and_then(|e| e.as_ref()) {
        Some(entry) => match &entry.kind {
            crate::vfs::FdKind::Epoll(inst) => Arc::clone(inst),
            _ => return SyscallResult::Error(9),
        },
        _ => return SyscallResult::Error(9),
    };
    drop(inner);

    match instance.ctl(op, fd, events, data) {
        Ok(()) => SyscallResult::Success(0),
        Err(e) => SyscallResult::Error(e as i64),
    }
}

pub fn handle_epoll_wait(args: SyscallArgs) -> SyscallResult {
    let epfd = args.arg0 as usize;
    let max_events = args.arg1 as usize;
    let _timeout_ms = args.arg2 as i32;

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let inner = process.inner.lock();
    let instance = match inner.fd_table.get(epfd).and_then(|e| e.as_ref()) {
        Some(entry) => match &entry.kind {
            crate::vfs::FdKind::Epoll(inst) => Arc::clone(inst),
            _ => return SyscallResult::Error(9),
        },
        _ => return SyscallResult::Error(9),
    };
    drop(inner);

    let ready = instance.wait(max_events, _timeout_ms);
    SyscallResult::Success(ready.len() as u64)
}

// ---------------------------------------------------------------------------
// EventFD syscalls
// ---------------------------------------------------------------------------

pub fn handle_eventfd_create(args: SyscallArgs) -> SyscallResult {
    let initval = args.arg0;

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let efd = alloc::sync::Arc::new(crate::ipc::eventfd::EventFd::new(initval));

    let fd = helper_alloc_fd(
        &process,
        crate::vfs::FileDescriptor {
            inode: crate::vfs::InodeId(0),
            backend: alloc::sync::Arc::new(crate::fs::tmpfs::TmpfsBackend::new()),
            offset: core::sync::atomic::AtomicU64::new(0),
            flags: crate::vfs::OpenFlags::RDWR,
            kind: crate::vfs::FdKind::EventFd(efd),
            name: alloc::string::String::from("eventfd"),
        },
    );

    SyscallResult::Success(fd as u64)
}

pub fn handle_eventfd_read(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let inner = process.inner.lock();
    let efd = match inner.fd_table.get(fd).and_then(|e| e.as_ref()) {
        Some(fd_entry) => match &fd_entry.kind {
            crate::vfs::FdKind::EventFd(efd) => alloc::sync::Arc::clone(efd),
            _ => return SyscallResult::Error(9), // EBADF (not an eventfd)
        },
        None => return SyscallResult::Error(9),
    };
    drop(inner);

    match efd.read_value() {
        Ok(val) => SyscallResult::Success(val),
        Err(_) => SyscallResult::Error(11), // EAGAIN (would block)
    }
}

pub fn handle_eventfd_write(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let val = args.arg1;

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let inner = process.inner.lock();
    let efd = match inner.fd_table.get(fd).and_then(|e| e.as_ref()) {
        Some(fd_entry) => match &fd_entry.kind {
            crate::vfs::FdKind::EventFd(efd) => alloc::sync::Arc::clone(efd),
            _ => return SyscallResult::Error(9),
        },
        None => return SyscallResult::Error(9),
    };
    drop(inner);

    match efd.write_value(val) {
        Ok(()) => SyscallResult::Success(0),
        Err(_) => SyscallResult::Error(11), // EAGAIN (would block)
    }
}

// ---------------------------------------------------------------------------
// TimerFD syscalls
// ---------------------------------------------------------------------------

pub fn handle_timerfd_create(_args: SyscallArgs) -> SyscallResult {
    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let tfd = alloc::sync::Arc::new(crate::ipc::timerfd::TimerFd::new());
    crate::ipc::timerfd::register_timerfd(alloc::sync::Arc::clone(&tfd));

    let fd = helper_alloc_fd(
        &process,
        crate::vfs::FileDescriptor {
            inode: crate::vfs::InodeId(0),
            backend: alloc::sync::Arc::new(crate::fs::tmpfs::TmpfsBackend::new()),
            offset: core::sync::atomic::AtomicU64::new(0),
            flags: crate::vfs::OpenFlags::RDONLY,
            kind: crate::vfs::FdKind::TimerFd(tfd),
            name: alloc::string::String::from("timerfd"),
        },
    );

    SyscallResult::Success(fd as u64)
}

pub fn handle_timerfd_settime(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let initial_ticks = args.arg1;
    let interval_ticks = args.arg2;
    let flags = args.arg3 as u32;

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let inner = process.inner.lock();
    let tfd = match inner.fd_table.get(fd).and_then(|e| e.as_ref()) {
        Some(fd_entry) => match &fd_entry.kind {
            crate::vfs::FdKind::TimerFd(tfd) => alloc::sync::Arc::clone(tfd),
            _ => return SyscallResult::Error(9),
        },
        None => return SyscallResult::Error(9),
    };
    drop(inner);

    tfd.settime(initial_ticks, interval_ticks, flags);
    SyscallResult::Success(0)
}

pub fn handle_timerfd_gettime(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let inner = process.inner.lock();
    let tfd = match inner.fd_table.get(fd).and_then(|e| e.as_ref()) {
        Some(fd_entry) => match &fd_entry.kind {
            crate::vfs::FdKind::TimerFd(tfd) => alloc::sync::Arc::clone(tfd),
            _ => return SyscallResult::Error(9),
        },
        None => return SyscallResult::Error(9),
    };
    drop(inner);

    let (remaining, interval) = tfd.gettime();
    // Encode remaining in low 32 bits, interval in high 32 bits.
    SyscallResult::Success((remaining & 0xFFFF_FFFF) | (interval << 32))
}

// ---------------------------------------------------------------------------
// Futex syscall
// ---------------------------------------------------------------------------

pub fn handle_futex(args: SyscallArgs) -> SyscallResult {
    let uaddr = args.arg0;
    let op = args.arg1 as u32;
    let val = args.arg2 as u32;

    match op {
        crate::ipc::futex::FUTEX_WAIT => match crate::ipc::futex::futex_wait(uaddr, val) {
            Ok(()) => SyscallResult::Success(0),
            Err(e) => SyscallResult::Error(e as i64),
        },
        crate::ipc::futex::FUTEX_WAKE => match crate::ipc::futex::futex_wake(uaddr, val as usize) {
            Ok(n) => SyscallResult::Success(n as u64),
            Err(e) => SyscallResult::Error(e as i64),
        },
        _ => SyscallResult::Error(22), // EINVAL
    }
}
