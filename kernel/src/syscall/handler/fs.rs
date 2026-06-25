use super::{FSTYPE_EXT2, FSTYPE_EXT4, FSTYPE_TMPFS, SyscallResult};
use crate::fs::ext4::Ext4Backend;
use crate::fs::tmpfs::TmpfsBackend;
use crate::fs::vfs::{FsBackend, MountFlags};
use crate::vfs::VFS;
use alloc::sync::Arc;
use turnix_abi::syscall::SyscallArgs;

pub fn handle_mkdir(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(1);
    }
    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let path_str = core::str::from_utf8(path_slice).unwrap_or("");
    let mut vfs = VFS.lock();
    if vfs.mkdir(path_str) {
        SyscallResult::Success(0)
    } else {
        SyscallResult::Error(1)
    }
}

pub fn handle_unlink(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(1);
    }
    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let path_str = core::str::from_utf8(path_slice).unwrap_or("");
    let mut vfs = VFS.lock();
    if vfs.unlink(path_str) {
        SyscallResult::Success(0)
    } else {
        SyscallResult::Error(1)
    }
}

pub fn handle_write_file(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let buf_ptr = args.arg1 as *const u8;
    let buf_len = args.arg2 as usize;

    if buf_ptr.is_null() || buf_len == 0 {
        return SyscallResult::Error(1);
    }

    let buf = unsafe { core::slice::from_raw_parts(buf_ptr, buf_len) };

    // For pipe FDs, extract the Arc<PipeBuffer> and perform a blocking
    // write outside the VFS lock so the reader can make progress.
    let pipe_buf = {
        let vfs = VFS.lock();
        vfs.get_pipe_buffer(fd)
    };
    if let Some(pb) = pipe_buf {
        let n = pb.write_blocking(buf);
        return SyscallResult::Success(n as u64);
    }

    let mut vfs = VFS.lock();
    match vfs.write(fd, buf) {
        Some(len) => SyscallResult::Success(len as u64),
        None => SyscallResult::Error(1),
    }
}

pub fn handle_seek(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let offset = args.arg1 as i64;
    let whence = args.arg2 as i32;

    let mut vfs = VFS.lock();

    let new_offset = match whence {
        0 => offset as u64, // SEEK_SET
        1 => {
            let cur = match vfs.get_fd_offset(fd) {
                Some(o) => o,
                None => return SyscallResult::Error(9), // EBADF
            };
            cur.wrapping_add(offset as u64) // SEEK_CUR
        }
        2 => {
            let size = match vfs.stat_fd(fd) {
                Some(s) => s,
                None => return SyscallResult::Error(9), // EBADF
            };
            size.wrapping_add(offset as u64) // SEEK_END
        }
        _ => return SyscallResult::Error(22), // EINVAL
    };

    if vfs.seek(fd, new_offset) {
        SyscallResult::Success(new_offset)
    } else {
        SyscallResult::Error(9) // EBADF
    }
}

pub fn handle_stat(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let stat_ptr = args.arg2 as *mut turnix_abi::syscall::Stat;

    if path_ptr.is_null() || path_len == 0 || stat_ptr.is_null() {
        return SyscallResult::Error(1);
    }

    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let path = core::str::from_utf8(path_slice).unwrap_or("");

    let vfs = VFS.lock();
    match vfs.stat(path) {
        Some(stat) => {
            unsafe {
                *stat_ptr = stat.to_abi();
            }
            SyscallResult::Success(0)
        }
        None => SyscallResult::Error(1),
    }
}

pub fn handle_ls(args: SyscallArgs) -> SyscallResult {
    let buf_ptr = args.arg0 as *mut u8;
    let buf_len = args.arg1 as usize;

    if buf_ptr.is_null() || buf_len == 0 {
        return SyscallResult::Error(1);
    }

    let vfs = VFS.lock();
    let files = vfs.list_dir();
    let mut offset = 0;
    let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr, buf_len) };

    for name in files {
        let name_bytes = name.as_bytes();
        if offset + name_bytes.len() + 1 > buf_len {
            break;
        }
        buf[offset..offset + name_bytes.len()].copy_from_slice(name_bytes);
        offset += name_bytes.len();
        buf[offset] = b'\n';
        offset += 1;
    }

    SyscallResult::Success(offset as u64)
}

pub fn handle_open(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let flags_bits = args.arg2 as u32;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(1);
    }

    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let path = core::str::from_utf8(path_slice).unwrap_or("");

    let mut vfs = VFS.lock();
    match vfs.open_with_creds(path, crate::fs::vfs::OpenFlags(flags_bits), 0, 0) {
        Ok(fd) => SyscallResult::Success(fd as u64),
        Err(_) => SyscallResult::Error(1),
    }
}

pub fn handle_close(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;

    // Try network socket table first
    if crate::net::socket::sys_close(fd).is_ok() {
        return SyscallResult::Success(0);
    }

    // Fall back to VFS
    let mut vfs = VFS.lock();
    if vfs.close(fd) {
        SyscallResult::Success(0)
    } else {
        SyscallResult::Error(9) // EBADF
    }
}

/// `pipe(pipefd: *mut [u64; 2]) -> 0 on success`
///
/// Creates a unidirectional data pipe.  `pipefd[0]` receives the read end fd,
/// `pipefd[1]` receives the write end fd.
pub fn handle_pipe(args: SyscallArgs) -> SyscallResult {
    let pipefd_ptr = args.arg0 as *mut [u64; 2];

    if pipefd_ptr.is_null() {
        return SyscallResult::Error(14); // EFAULT
    }

    let mut vfs = VFS.lock();
    let (read_idx, write_idx) = vfs.create_pipe();

    let pipefds = [read_idx as u64, write_idx as u64];
    unsafe {
        pipefd_ptr.write(pipefds);
    }

    SyscallResult::Success(0)
}

/// `dup(oldfd: i32) -> newfd`
pub fn handle_dup(args: SyscallArgs) -> SyscallResult {
    let oldfd = args.arg0 as usize;
    let mut vfs = VFS.lock();
    match vfs.dup_fd(oldfd) {
        Some(newfd) => SyscallResult::Success(newfd as u64),
        None => SyscallResult::Error(9), // EBADF
    }
}

/// `dup2(oldfd: i32, newfd: i32) -> newfd`
pub fn handle_dup2(args: SyscallArgs) -> SyscallResult {
    let oldfd = args.arg0 as usize;
    let newfd = args.arg1 as usize;
    if newfd > 1023 {
        return SyscallResult::Error(22); // EINVAL
    }
    let mut vfs = VFS.lock();
    match vfs.dup2_fd(oldfd, newfd) {
        Some(fd) => SyscallResult::Success(fd as u64),
        None => SyscallResult::Error(9), // EBADF
    }
}

pub fn handle_mount(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let fs_type = args.arg2;
    // arg3: flags (reserved / future use — ignored for now)

    // Check CAP_SYS_ADMIN
    if let Some(current) = crate::task::scheduler::get_current_process()
        && !current
            .inner
            .lock()
            .sec_ctx
            .has_capability(crate::security::capabilities::Capability::SysAdmin)
    {
        return SyscallResult::Error(1); // EPERM
    }

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(22); // EINVAL
    }

    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let mount_point = match core::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => return SyscallResult::Error(22),
    };

    let backend: Arc<dyn FsBackend> = match fs_type {
        FSTYPE_TMPFS => Arc::new(TmpfsBackend::new()),
        FSTYPE_EXT4 => Arc::new(Ext4Backend::new()),
        FSTYPE_EXT2 => {
            // ext2 requires a device id; we default to device 0 here.
            // A more complete ABI would pass the device id in arg3.
            let backend = crate::fs::ext2::Ext2Backend::new(0);
            Arc::new(backend)
        }
        _ => return SyscallResult::Error(22), // EINVAL — unknown fs type
    };

    let mut vfs = VFS.lock();
    match vfs.mount(mount_point, backend, MountFlags::default()) {
        Ok(()) => SyscallResult::Success(0),
        Err(e) => {
            crate::serial::println!("[mount] failed at '{}': {:?}", mount_point, e);
            SyscallResult::Error(1)
        }
    }
}

pub fn handle_umount(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(22); // EINVAL
    }

    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let mount_point = match core::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => return SyscallResult::Error(22),
    };

    let mut vfs = VFS.lock();
    match vfs.umount(mount_point) {
        Ok(()) => SyscallResult::Success(0),
        Err(crate::fs::vfs::FsError::BusyMounted) => SyscallResult::Error(16), // EBUSY
        Err(crate::fs::vfs::FsError::NotFound) => SyscallResult::Error(2),     // ENOENT
        Err(e) => {
            crate::serial::println!("[umount] failed at '{}': {:?}", mount_point, e);
            SyscallResult::Error(1)
        }
    }
}

pub fn handle_ftruncate(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let size = args.arg1;

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let (inode, backend) = {
        let inner = process.inner.lock();
        let fd_entry = match inner.fd_table.get(fd).and_then(|e| e.as_ref()) {
            Some(e) => e.clone(),
            None => return SyscallResult::Error(9), // EBADF
        };
        (fd_entry.inode, fd_entry.backend)
    };

    match backend.truncate(inode, size) {
        Ok(()) => SyscallResult::Success(0),
        Err(_) => SyscallResult::Error(22), // EINVAL
    }
}

pub fn handle_chdir(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(22); // EINVAL
    }

    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let path = match core::str::from_utf8(path_slice) {
        Ok(p) => p,
        Err(_) => return SyscallResult::Error(22), // EINVAL
    };

    // Resolve relative paths against the current working directory.
    let absolute_path = if path.starts_with('/') {
        alloc::string::String::from(path)
    } else {
        let cwd = match crate::task::scheduler::get_current_process() {
            Some(p) => p.inner.lock().cwd.clone(),
            None => return SyscallResult::Error(1), // EPERM
        };
        let mut combined = cwd;
        if !combined.ends_with('/') {
            combined.push('/');
        }
        combined.push_str(path);
        combined
    };

    // Normalize the path: resolve "." and ".." components, collapse separators.
    let normalized = normalize_path(&absolute_path);
    if normalized.is_empty() {
        return SyscallResult::Error(22); // EINVAL
    }

    // Verify the path exists and is a directory.
    {
        let vfs = crate::vfs::VFS.lock();
        match vfs.stat(&normalized) {
            Some(stat) => {
                if stat.file_type != crate::fs::vfs::FileType::Directory {
                    return SyscallResult::Error(20); // ENOTDIR
                }
            }
            None => return SyscallResult::Error(2), // ENOENT
        }
    }

    // Update the current process's cwd.
    if let Some(process) = crate::task::scheduler::get_current_process() {
        let mut inner = process.inner.lock();
        inner.cwd = normalized;
        SyscallResult::Success(0)
    } else {
        SyscallResult::Error(1) // EPERM
    }
}

/// Normalize a filesystem path by resolving `.` (current dir) and `..` (parent dir)
/// components and collapsing redundant separators.
pub(crate) fn normalize_path(path: &str) -> alloc::string::String {
    use alloc::vec::Vec;

    let mut components: Vec<&str> = Vec::new();

    for component in path.split('/') {
        match component {
            "" | "." => continue,
            ".." => {
                components.pop();
            }
            other => components.push(other),
        }
    }

    let mut result = alloc::string::String::new();
    for comp in &components {
        result.push('/');
        result.push_str(comp);
    }

    if result.is_empty() {
        result.push('/');
    }

    result
}

pub fn handle_xattr_get(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let name_ptr = args.arg2 as *const u8;
    let name_len = args.arg3 as usize;

    if path_ptr.is_null() || path_len == 0 || name_ptr.is_null() || name_len == 0 {
        return SyscallResult::Error(22); // EINVAL
    }

    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let path = match core::str::from_utf8(path_slice) {
        Ok(p) => p,
        Err(_) => return SyscallResult::Error(22),
    };
    let name_slice = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let name = match core::str::from_utf8(name_slice) {
        Ok(n) => n,
        Err(_) => return SyscallResult::Error(22),
    };

    let vfs = VFS.lock();
    match vfs.xattr_get(path, name) {
        Ok(Some(data)) => {
            // Return the data size; caller must supply a buffer via a
            // follow-up read or the ABI should be extended.  For now
            // we return the length so userspace can allocate.
            SyscallResult::Success(data.len() as u64)
        }
        Ok(None) => SyscallResult::Error(61), // ENODATA
        Err(_) => SyscallResult::Error(2),    // ENOENT
    }
}

pub fn handle_xattr_set(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let name_ptr = args.arg2 as *const u8;
    let name_len = args.arg3 as usize;

    if path_ptr.is_null() || path_len == 0 || name_ptr.is_null() || name_len == 0 {
        return SyscallResult::Error(22); // EINVAL
    }

    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let _path = match core::str::from_utf8(path_slice) {
        Ok(p) => p,
        Err(_) => return SyscallResult::Error(22),
    };
    let name_slice = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let _name = match core::str::from_utf8(name_slice) {
        Ok(n) => n,
        Err(_) => return SyscallResult::Error(22),
    };

    // Check CAP_SETFCAP for setting security xattrs
    if let Some(current) = crate::task::scheduler::get_current_process()
        && !current
            .inner
            .lock()
            .sec_ctx
            .has_capability(crate::security::capabilities::Capability::Setfcap)
    {
        return SyscallResult::Error(1); // EPERM
    }

    // Value pointer and length are packed in a second arg pair.
    // For now, the ABI uses arg2/arg3 for the name; the value
    // is passed through a separate mechanism (future extension).
    // Stub: return success for now.
    let _vfs = VFS.lock();
    SyscallResult::Success(0)
}

pub fn handle_write(args: SyscallArgs) -> SyscallResult {
    let addr = args.arg0 as *const u8;
    let len = args.arg1 as usize;

    if addr.is_null() || len == 0 {
        return SyscallResult::Error(1);
    }

    // Safety: In a real OS we'd verify this address belongs to the user
    let slice = unsafe { core::slice::from_raw_parts(addr, len) };

    let string = core::str::from_utf8(slice).unwrap_or("");
    crate::serial::print(format_args!("{}", string));

    SyscallResult::Success(len as u64)
}

pub fn handle_read(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let buf_ptr = args.arg1 as *mut u8;
    let buf_len = args.arg2 as usize;

    if buf_ptr.is_null() || buf_len == 0 {
        return SyscallResult::Error(1);
    }

    let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr, buf_len) };
    let mut vfs = VFS.lock();
    match vfs.read(fd, buf) {
        Some(len) => SyscallResult::Success(len as u64),
        None => SyscallResult::Error(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_path_absolute_simple() {
        assert_eq!(normalize_path("/foo/bar"), "/foo/bar");
    }

    #[test]
    fn normalize_path_dot_resolution() {
        assert_eq!(normalize_path("/foo/./bar"), "/foo/bar");
    }

    #[test]
    fn normalize_path_double_dot() {
        assert_eq!(normalize_path("/foo/bar/../baz"), "/foo/baz");
    }

    #[test]
    fn normalize_path_double_dot_to_root() {
        assert_eq!(normalize_path("/foo/.."), "/");
    }

    #[test]
    fn normalize_path_multiple_slashes() {
        assert_eq!(normalize_path("/foo//bar///baz"), "/foo/bar/baz");
    }

    #[test]
    fn normalize_path_leading_trailing_slashes() {
        assert_eq!(normalize_path("///foo/bar///"), "/foo/bar");
    }

    #[test]
    fn normalize_path_empty_gives_root() {
        assert_eq!(normalize_path(""), "/");
    }

    #[test]
    fn normalize_path_root_only() {
        assert_eq!(normalize_path("/"), "/");
    }

    #[test]
    fn normalize_path_deep_dotdot() {
        assert_eq!(normalize_path("/a/b/c/../../d"), "/a/d");
    }

    #[test]
    fn normalize_path_dotdot_past_root_stays_at_root() {
        assert_eq!(normalize_path("/../../../foo"), "/foo");
    }

    #[test]
    fn handle_chdir_null_ptr_returns_einval() {
        let args = SyscallArgs::new(0, 0, 0, 0);
        let result = handle_chdir(args);
        assert!(matches!(result, SyscallResult::Error(22)));
    }

    #[test]
    fn handle_chdir_zero_len_returns_einval() {
        let path = "/tmp";
        let args = SyscallArgs::new(path.as_ptr() as u64, 0, 0, 0);
        let result = handle_chdir(args);
        assert!(matches!(result, SyscallResult::Error(22)));
    }

    #[test]
    fn handle_chdir_nonexistent_path_returns_enoent() {
        let _guard = crate::test_serial::acquire();
        // Set up a minimal VFS with a tmpfs root
        {
            let mut vfs = crate::vfs::VFS.lock();
            *vfs = crate::vfs::Vfs::new();
            vfs.mount(
                "/",
                alloc::sync::Arc::new(crate::fs::tmpfs::TmpfsBackend::new()),
                crate::fs::vfs::MountFlags::default(),
            )
            .unwrap();
        }

        // Set up a current process with cwd="/"
        let process = crate::process::Process {
            inner: alloc::sync::Arc::new(spin::Mutex::new(
                crate::process::ProcessControlBlock {
                    id: crate::process::ProcessId(99),
                    ppid: crate::process::ProcessId(0),
                    state: crate::process::ProcessState::Running,
                    pml4_frame: x86_64::structures::paging::PhysFrame::containing_address(
                        x86_64::PhysAddr::new(0),
                    ),
                    entry_point: x86_64::VirtAddr::zero(),
                    stack_top: x86_64::VirtAddr::zero(),
                    threads: alloc::vec![],
                    vma_set: crate::memory::vma::VmaSet::new(),
                    mmap_next_addr: x86_64::VirtAddr::zero(),
                    aslr_base: x86_64::VirtAddr::zero(),
                    fd_table: alloc::vec![None; 1024],
                    signal_mask: crate::process::SignalSet::empty(),
                    signal_handlers: [crate::process::SignalAction::Default; 64],
                    pending_signals: crate::process::SignalSet::empty(),
                    pending_signal_frame: None,
                    sec_ctx: crate::security::SecurityContext::root(),
                    nsproxy: crate::security::namespaces::NsProxy::new(),
                    seccomp_filter: None,
                    cgroup_path: None,
                    cwd: alloc::string::String::from("/"),
                },
            )),
        };
        let task = crate::task::Task::new_test(
            crate::task::TaskId::new(),
            process,
            crate::task::TaskState::Running,
        );
        crate::task::scheduler::set_current_task_for_test(task);

        let path = "/nonexistent_dir";
        let args = SyscallArgs::new(path.as_ptr() as u64, path.len() as u64, 0, 0);
        let result = handle_chdir(args);
        assert!(matches!(result, SyscallResult::Error(2)), "Expected ENOENT for nonexistent path");
    }

    #[test]
    fn handle_chdir_to_file_returns_enotdir() {
        let _guard = crate::test_serial::acquire();
        {
            let mut vfs = crate::vfs::VFS.lock();
            *vfs = crate::vfs::Vfs::new();
            let backend = alloc::sync::Arc::new(crate::fs::tmpfs::TmpfsBackend::new());
            // Create a file directly in the tmpfs backend
            {
                let mut inner = backend.inner.lock();
                inner.create_file(crate::fs::vfs::InodeId(1), "regular_file", 0o644);
            }
            vfs.mount(
                "/",
                backend,
                crate::fs::vfs::MountFlags::default(),
            )
            .unwrap();
        }

        let process = crate::process::Process {
            inner: alloc::sync::Arc::new(spin::Mutex::new(
                crate::process::ProcessControlBlock {
                    id: crate::process::ProcessId(98),
                    ppid: crate::process::ProcessId(0),
                    state: crate::process::ProcessState::Running,
                    pml4_frame: x86_64::structures::paging::PhysFrame::containing_address(
                        x86_64::PhysAddr::new(0),
                    ),
                    entry_point: x86_64::VirtAddr::zero(),
                    stack_top: x86_64::VirtAddr::zero(),
                    threads: alloc::vec![],
                    vma_set: crate::memory::vma::VmaSet::new(),
                    mmap_next_addr: x86_64::VirtAddr::zero(),
                    aslr_base: x86_64::VirtAddr::zero(),
                    fd_table: alloc::vec![None; 1024],
                    signal_mask: crate::process::SignalSet::empty(),
                    signal_handlers: [crate::process::SignalAction::Default; 64],
                    pending_signals: crate::process::SignalSet::empty(),
                    pending_signal_frame: None,
                    sec_ctx: crate::security::SecurityContext::root(),
                    nsproxy: crate::security::namespaces::NsProxy::new(),
                    seccomp_filter: None,
                    cgroup_path: None,
                    cwd: alloc::string::String::from("/"),
                },
            )),
        };
        let task = crate::task::Task::new_test(
            crate::task::TaskId::new(),
            process,
            crate::task::TaskState::Running,
        );
        crate::task::scheduler::set_current_task_for_test(task);

        let path = "/regular_file";
        let args = SyscallArgs::new(path.as_ptr() as u64, path.len() as u64, 0, 0);
        let result = handle_chdir(args);
        assert!(matches!(result, SyscallResult::Error(20)), "Expected ENOTDIR for file path");
    }

    #[test]
    fn handle_chdir_success_updates_cwd() {
        let _guard = crate::test_serial::acquire();
        {
            let mut vfs = crate::vfs::VFS.lock();
            *vfs = crate::vfs::Vfs::new();
            vfs.mount(
                "/",
                alloc::sync::Arc::new(crate::fs::tmpfs::TmpfsBackend::new()),
                crate::fs::vfs::MountFlags::default(),
            )
            .unwrap();
            assert!(vfs.mkdir("/home"), "Failed to create /home");
            assert!(vfs.mkdir("/home/user"), "Failed to create /home/user");
        }

        let process = crate::process::Process {
            inner: alloc::sync::Arc::new(spin::Mutex::new(
                crate::process::ProcessControlBlock {
                    id: crate::process::ProcessId(97),
                    ppid: crate::process::ProcessId(0),
                    state: crate::process::ProcessState::Running,
                    pml4_frame: x86_64::structures::paging::PhysFrame::containing_address(
                        x86_64::PhysAddr::new(0),
                    ),
                    entry_point: x86_64::VirtAddr::zero(),
                    stack_top: x86_64::VirtAddr::zero(),
                    threads: alloc::vec![],
                    vma_set: crate::memory::vma::VmaSet::new(),
                    mmap_next_addr: x86_64::VirtAddr::zero(),
                    aslr_base: x86_64::VirtAddr::zero(),
                    fd_table: alloc::vec![None; 1024],
                    signal_mask: crate::process::SignalSet::empty(),
                    signal_handlers: [crate::process::SignalAction::Default; 64],
                    pending_signals: crate::process::SignalSet::empty(),
                    pending_signal_frame: None,
                    sec_ctx: crate::security::SecurityContext::root(),
                    nsproxy: crate::security::namespaces::NsProxy::new(),
                    seccomp_filter: None,
                    cgroup_path: None,
                    cwd: alloc::string::String::from("/"),
                },
            )),
        };
        let task = crate::task::Task::new_test(
            crate::task::TaskId::new(),
            process,
            crate::task::TaskState::Running,
        );
        crate::task::scheduler::set_current_task_for_test(task);

        let path = "/home/user";
        let args = SyscallArgs::new(path.as_ptr() as u64, path.len() as u64, 0, 0);
        let result = handle_chdir(args);
        assert!(matches!(result, SyscallResult::Success(0)), "Expected success for valid directory");

        // Verify CWD was updated
        let cwd = crate::task::scheduler::get_current_process()
            .unwrap()
            .inner
            .lock()
            .cwd
            .clone();
        assert_eq!(cwd, "/home/user", "CWD should be updated to /home/user");
    }

    #[test]
    fn normalize_path_complex() {
        assert_eq!(normalize_path("/a/b/../c/./d///e"), "/a/c/d/e");
        assert_eq!(normalize_path("/a/b/c/../../.."), "/");
        assert_eq!(normalize_path("//a///b/"), "/a/b");
        assert_eq!(normalize_path("/././."), "/");
        assert_eq!(normalize_path("/a/b/../../c/d/.."), "/c");
    }
}
