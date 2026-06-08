use crate::drivers::gpu;
use crate::fs::ext4::Ext4Backend;
use crate::fs::tmpfs::TmpfsBackend;
use crate::fs::vfs::{FsBackend, MountFlags};
use crate::memory::vma::{VmaFlags, VmaProt};
use crate::vfs::VFS;
use alloc::sync::Arc;
use turnix_abi::syscall::{Syscall, SyscallArgs, SyscallHeader};
use x86_64::VirtAddr;

#[derive(Debug)]
pub enum SyscallResult {
    Success(u64),
    Error(i64),
}

impl SyscallResult {
    pub fn is_success(&self) -> bool {
        matches!(self, SyscallResult::Success(_))
    }

    pub fn value(&self) -> u64 {
        match self {
            SyscallResult::Success(v) => *v,
            SyscallResult::Error(_) => 0,
        }
    }
}

pub fn handle_syscall(syscall: Syscall, args: SyscallArgs) -> SyscallResult {
    match syscall {
        Syscall::Write => handle_write(args),
        Syscall::Read => handle_read(args),
        Syscall::Exit => handle_exit(args),
        Syscall::Open => handle_open(args),
        Syscall::Close => handle_close(args),
        Syscall::Exec => handle_exec(args),
        Syscall::Fork => handle_fork(args),
        Syscall::Wait => handle_wait(args),
        Syscall::Yielder => handle_yielder(args),
        Syscall::Uptime => handle_uptime(args),
        Syscall::Ls => handle_ls(args),
        Syscall::Stat => handle_stat(args),
        Syscall::GetPid => handle_getpid(args),
        Syscall::Seek => handle_seek(args),
        Syscall::WriteFile => handle_write_file(args),
        Syscall::GetUid => handle_getuid(args),
        Syscall::GetGid => handle_getgid(args),
        Syscall::Brk => handle_brk(args),
        Syscall::Mkdir => handle_mkdir(args),
        Syscall::Unlink => handle_unlink(args),
        Syscall::MmapFramebuffer => handle_mmap_framebuffer_syscall(args),
        Syscall::Mmap => handle_mmap(args),
        Syscall::Munmap => handle_munmap(args),
        Syscall::Mount => handle_mount(args),
        Syscall::Umount => handle_umount(args),
    }
}

fn handle_mkdir(args: SyscallArgs) -> SyscallResult {
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

fn handle_unlink(args: SyscallArgs) -> SyscallResult {
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

fn handle_brk(_args: SyscallArgs) -> SyscallResult {
    // Brk syscall for user space memory allocation
    // arg0: new break address (0 to get current)
    // Returns the new break address or 0 on error
    // For now, return error as this requires proper memory management
    SyscallResult::Error(1)
}

fn handle_getuid(_args: SyscallArgs) -> SyscallResult {
    // Return 0 for root user (no user management yet)
    SyscallResult::Success(0)
}

fn handle_getgid(_args: SyscallArgs) -> SyscallResult {
    // Return 0 for root group (no group management yet)
    SyscallResult::Success(0)
}

fn handle_write_file(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let buf_ptr = args.arg1 as *const u8;
    let buf_len = args.arg2 as usize;

    if buf_ptr.is_null() || buf_len == 0 {
        return SyscallResult::Error(1);
    }

    let buf = unsafe { core::slice::from_raw_parts(buf_ptr, buf_len) };
    let mut vfs = VFS.lock();
    match vfs.write(fd, buf) {
        Some(len) => SyscallResult::Success(len as u64),
        None => SyscallResult::Error(1),
    }
}

fn handle_seek(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let offset = args.arg1;
    let mut vfs = VFS.lock();
    if vfs.seek(fd, offset) {
        SyscallResult::Success(0)
    } else {
        SyscallResult::Error(1)
    }
}

fn handle_getpid(_args: SyscallArgs) -> SyscallResult {
    match crate::task::scheduler::get_current_task_id() {
        Some(tid) => SyscallResult::Success(tid.as_usize() as u64),
        None => SyscallResult::Error(1),
    }
}

fn handle_uptime(_args: SyscallArgs) -> SyscallResult {
    let ticks = crate::task::scheduler::get_uptime_ticks();
    SyscallResult::Success(ticks)
}

fn handle_write(args: SyscallArgs) -> SyscallResult {
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

fn handle_read(args: SyscallArgs) -> SyscallResult {
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

fn handle_exit(args: SyscallArgs) -> SyscallResult {
    let code = args.arg0 as i32;
    crate::serial::print(format_args!("\n[syscall] exit code: {}\n", code));
    crate::task::scheduler::exit_current_task();
    // This line is unreachable but required for the function signature
    #[allow(unreachable_code)]
    SyscallResult::Success(0)
}

fn handle_open(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(1);
    }

    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let path = core::str::from_utf8(path_slice).unwrap_or("");

    let mut vfs = VFS.lock();
    match vfs.open(path) {
        Some(fd) => SyscallResult::Success(fd as u64),
        None => SyscallResult::Error(1),
    }
}

fn handle_close(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let mut vfs = VFS.lock();
    vfs.close(fd);
    SyscallResult::Success(0)
}

fn handle_ls(args: SyscallArgs) -> SyscallResult {
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

fn handle_stat(args: SyscallArgs) -> SyscallResult {
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

fn handle_exec(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let argv_ptr = args.arg2 as *const *const u8;
    let envp_ptr = args.arg3 as *const *const u8;

    // Validate path pointer and len
    if path_ptr.is_null() || path_len == 0 || path_len > 4096 {
        return SyscallResult::Error(1);
    }

    // Convert user path to &str
    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let path = match core::str::from_utf8(path_slice) {
        Ok(p) => p,
        Err(_) => return SyscallResult::Error(2), // Invalid UTF-8
    };

    // Collect argv strings
    let mut argv = alloc::vec![];
    if !argv_ptr.is_null() {
        let mut i = 0;
        loop {
            let str_ptr = unsafe { *argv_ptr.add(i) };
            if str_ptr.is_null() {
                break;
            }
            // Get string length (find null terminator)
            let mut len = 0;
            while unsafe { *str_ptr.add(len) } != 0 {
                len += 1;
                if len > 4096 {
                    return SyscallResult::Error(2);
                }
            }
            let str_slice = unsafe { core::slice::from_raw_parts(str_ptr, len) };
            argv.push(str_slice);
            i += 1;
        }
    } else {
        // Default argv: [program name]
        argv.push(path_slice);
    }

    // Collect envp strings
    let mut envp = alloc::vec![];
    if !envp_ptr.is_null() {
        let mut i = 0;
        loop {
            let str_ptr = unsafe { *envp_ptr.add(i) };
            if str_ptr.is_null() {
                break;
            }
            // Get string length (find null terminator)
            let mut len = 0;
            while unsafe { *str_ptr.add(len) } != 0 {
                len += 1;
                if len > 4096 {
                    return SyscallResult::Error(2);
                }
            }
            let str_slice = unsafe { core::slice::from_raw_parts(str_ptr, len) };
            envp.push(str_slice);
            i += 1;
        }
    }

    // Get current process
    let current_process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(3), // No current process
    };

    // Open the file from VFS
    let mut vfs = crate::vfs::VFS.lock();
    let fd = match vfs.open(path) {
        Some(fd) => fd,
        None => return SyscallResult::Error(4), // File not found
    };

    // Get file stat to know size
    let stat = match vfs.stat(path) {
        Some(s) => s,
        None => {
            vfs.close(fd);
            return SyscallResult::Error(5);
        }
    };

    // Allocate buffer for ELF data
    let mut elf_data = alloc::vec![0u8; stat.size as usize];

    // Read the entire file into the buffer
    let read_len = match vfs.read(fd, &mut elf_data) {
        Some(len) => len,
        None => {
            vfs.close(fd);
            return SyscallResult::Error(6);
        }
    };

    // Close the file
    vfs.close(fd);

    // Drop VFS lock before doing process operations
    drop(vfs);

    // Get frame allocator and phys mem offset
    let mut frame_allocator_guard = crate::boot::get_frame_allocator().lock();
    let frame_allocator = match frame_allocator_guard.as_mut() {
        Some(fa) => fa,
        None => return SyscallResult::Error(7),
    };
    let phys_mem_offset = crate::boot::get_phys_mem_offset();

    // Perform exec on the current process
    match current_process.exec_from_elf(
        &elf_data[..read_len],
        &argv,
        &envp,
        frame_allocator,
        phys_mem_offset,
    ) {
        Ok(_) => (),
        Err(_) => return SyscallResult::Error(8),
    };

    // Drop frame allocator guard to unlock it
    drop(frame_allocator_guard);

    if crate::task::scheduler::with_current_task_mut(|task| task.switch_to()).is_none() {
        return SyscallResult::Error(9);
    }

    SyscallResult::Success(0)
}

pub fn handle_fork_with_frame(frame: &crate::arch::x86_64::syscall_arch::SyscallFrame) -> u64 {
    // 1. Get the current (parent) process
    let parent_process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return !0, // error
    };

    // 2. Get access to frame allocator and physical memory offset
    let mut frame_allocator_guard = crate::boot::get_frame_allocator().lock();
    let frame_allocator = match frame_allocator_guard.as_mut() {
        Some(fa) => fa,
        None => return !0, // error
    };
    let phys_mem_offset = crate::boot::get_phys_mem_offset();

    // 3. Create child process via Process::fork
    let child_process = parent_process.fork(frame_allocator, phys_mem_offset);
    let child_pid = child_process.id().0 as u64;

    // 4. Drop frame allocator guard before we try to get the mapper, since it's holding the lock
    drop(frame_allocator_guard);

    // 5. Create a new kernel task for the child process using new_forked_user
    // To get a mapper, we need to get the current kernel page table
    let (kernel_pml4_frame, _) = x86_64::registers::control::Cr3::read();
    let mut mapper = unsafe {
        let pml4_ptr = (phys_mem_offset + kernel_pml4_frame.start_address().as_u64())
            .as_mut_ptr::<x86_64::structures::paging::PageTable>();
        x86_64::structures::paging::OffsetPageTable::new(&mut *pml4_ptr, phys_mem_offset)
    };

    // Lock frame allocator again
    let mut frame_allocator_guard2 = crate::boot::get_frame_allocator().lock();
    let frame_allocator2 = frame_allocator_guard2.as_mut().unwrap();

    let child_task = crate::task::Task::new_forked_user(
        child_process.clone(),
        frame,
        &mut mapper,
        frame_allocator2,
        phys_mem_offset,
    );

    // 6. Add child process to PROCESS_TABLE
    {
        let mut process_table = crate::process::PROCESS_TABLE.lock();
        process_table.insert(child_process.id(), child_process.inner.clone());
    }

    // 7. Add the child task to the scheduler
    crate::task::scheduler::add_task(child_task);

    // 8. Return child PID to parent
    child_pid
}

fn handle_fork(_args: SyscallArgs) -> SyscallResult {
    // This is just a placeholder; the real implementation is in handle_fork_with_frame
    SyscallResult::Error(0)
}

fn handle_wait(_args: SyscallArgs) -> SyscallResult {
    // Basic wait implementation
    // In a real wait, we would:
    // 1. Wait for a child process to exit
    // 2. Return the PID and exit status
    // For now, return error
    SyscallResult::Error(1)
}

fn handle_yielder(args: SyscallArgs) -> SyscallResult {
    if args.arg0 != 0 {
        crate::task::scheduler::yield_task();
    }
    SyscallResult::Success(0)
}

fn handle_mmap_framebuffer_syscall(args: SyscallArgs) -> SyscallResult {
    let caller_pid = args.arg0;
    match gpu::handle_mmap_framebuffer(caller_pid) {
        Ok(addr) => SyscallResult::Success(addr),
        Err(_) => SyscallResult::Error(-1),
    }
}

fn handle_mmap(args: SyscallArgs) -> SyscallResult {
    let addr_hint = args.arg0;
    let length = args.arg1;
    let prot_bits = args.arg2 as u8;
    let flags_bits = args.arg3 as u8;

    if length == 0 || length > 0x1000_0000 {
        return SyscallResult::Error(22);
    }

    let prot = VmaProt::from_bits_truncate(prot_bits);
    if crate::memory::demand::check_wx(prot) {
        crate::serial::println!(
            "[mmap] W^X violation: rejecting MAP_ANONYMOUS with PROT_WRITE|PROT_EXEC"
        );
        return SyscallResult::Error(13);
    }

    let mut flags = VmaFlags::empty();
    if flags_bits & 1 != 0 {
        flags |= VmaFlags::MAP_PRIVATE;
    }
    if flags_bits & 2 != 0 {
        flags |= VmaFlags::MAP_SHARED;
    }
    if flags_bits & 4 != 0 {
        flags |= VmaFlags::MAP_FIXED;
    }

    let is_anon = flags_bits & 8 != 0;
    if !is_anon {
        return SyscallResult::Error(22);
    }

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let addr = if flags.contains(VmaFlags::MAP_FIXED) {
        Some(VirtAddr::new(addr_hint))
    } else {
        None
    };

    match process.mmap_anon(addr, length, prot, flags) {
        Ok(start) => SyscallResult::Success(start.as_u64()),
        Err(_) => SyscallResult::Error(11),
    }
}

fn handle_munmap(args: SyscallArgs) -> SyscallResult {
    let addr = args.arg0;
    let length = args.arg1;

    if length == 0 {
        return SyscallResult::Success(0);
    }

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    match process.munmap_range(VirtAddr::new(addr), length) {
        Ok(()) => SyscallResult::Success(0),
        Err(_) => SyscallResult::Error(1),
    }
}

/// Filesystem type constants for the `mount` syscall (arg2).
const FSTYPE_TMPFS: u64 = 0;
const FSTYPE_EXT2: u64 = 1;
const FSTYPE_EXT4: u64 = 2;

fn handle_mount(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let fs_type = args.arg2;
    // arg3: flags (reserved / future use — ignored for now)

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

fn handle_umount(args: SyscallArgs) -> SyscallResult {
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

pub fn syscall_from_user(header: SyscallHeader, args: SyscallArgs) -> SyscallResult {
    let syscall = match Syscall::from_u16(header.number) {
        Some(s) => s,
        None => {
            crate::serial::print(format_args!("unknown syscall: {}\n", header.number));
            return SyscallResult::Error(-1);
        }
    };

    handle_syscall(syscall, args)
}
