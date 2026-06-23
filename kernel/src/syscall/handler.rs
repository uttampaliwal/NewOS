use crate::drivers::gpu;
use crate::fs::ext4::Ext4Backend;
use crate::fs::tmpfs::TmpfsBackend;
use crate::fs::vfs::{FsBackend, MountFlags, UnixSocketState};
use crate::memory::vma::{VmaFlags, VmaProt};
use crate::process::SignalAction;
use crate::vfs::VFS;
use alloc::sync::Arc;
use turnix_abi::input::InputEvent;
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
        Syscall::Clone => handle_clone(args),
        Syscall::Wait => handle_wait(args),
        Syscall::Waitpid => handle_waitpid(args),
        Syscall::Yielder => handle_yielder(args),
        Syscall::Uptime => handle_uptime(args),
        Syscall::Ls => handle_ls(args),
        Syscall::Stat => handle_stat(args),
        Syscall::GetPid => handle_getpid(args),
        Syscall::Seek => handle_seek(args),
        Syscall::WriteFile => handle_write_file(args),
        Syscall::GetUid => handle_getuid(args),
        Syscall::GetGid => handle_getgid(args),
        Syscall::SetUid => handle_setuid(args),
        Syscall::SetGid => handle_setgid(args),
        Syscall::Brk => handle_brk(args),
        Syscall::Mkdir => handle_mkdir(args),
        Syscall::Unlink => handle_unlink(args),
        Syscall::MmapFramebuffer => handle_mmap_framebuffer_syscall(args),
        Syscall::Mmap => handle_mmap(args),
        Syscall::Munmap => handle_munmap(args),
        Syscall::Mount => handle_mount(args),
        Syscall::Umount => handle_umount(args),
        Syscall::Pipe => handle_pipe(args),
        Syscall::Socket => handle_socket(args),
        Syscall::Bind => handle_bind(args),
        Syscall::Listen => handle_listen(args),
        Syscall::Accept => handle_accept(args),
        Syscall::Connect => handle_connect(args),
        Syscall::Sigaction => handle_sigaction(args),
        Syscall::Sigprocmask => handle_sigprocmask(args),
        Syscall::Sigreturn => {
            // Sigreturn is handled specially via handle_sigreturn_with_frame
            // in syscall_dispatch. This arm should not be reached.
            SyscallResult::Success(0)
        }
        Syscall::Kill => handle_kill(args),
        Syscall::Dup => handle_dup(args),
        Syscall::Dup2 => handle_dup2(args),
        Syscall::Shutdown => handle_shutdown(args),
        Syscall::ReadShutdownSignal => handle_read_shutdown_signal(args),
        Syscall::Capget => handle_capget(args),
        Syscall::Capset => handle_capset(args),
        Syscall::Prctl => handle_prctl(args),
        Syscall::InputRead => handle_input_read(args),
        Syscall::GbmCreate => handle_gbm_create(args),
        Syscall::GbmMap => handle_gbm_map(args),
        Syscall::GbmDestroy => handle_gbm_destroy(args),
        Syscall::DrmPageFlip => handle_drm_page_flip(args),
        Syscall::Chdir => handle_chdir(args),
        Syscall::Dmesg => handle_dmesg(args),
        Syscall::XattrGet => handle_xattr_get(args),
        Syscall::XattrSet => handle_xattr_set(args),
        Syscall::NetSetAddr => handle_net_set_addr(args),
        Syscall::NetSetRoute => handle_net_set_route(args),
        Syscall::NetQuery => handle_net_query(args),
        Syscall::Ftruncate => handle_ftruncate(args),
        Syscall::Mmap2 => handle_mmap2(args),
        Syscall::ShmOpen => handle_shm_open(args),
        Syscall::ShmUnlink => handle_shm_unlink(args),
        Syscall::MqOpen => handle_mq_open(args),
        Syscall::MqClose => handle_mq_close(args),
        Syscall::MqUnlink => handle_mq_unlink(args),
        Syscall::MqSend => handle_mq_send(args),
        Syscall::MqReceive => handle_mq_receive(args),
        Syscall::Futex => handle_futex(args),
        Syscall::EpollCreate => handle_epoll_create(args),
        Syscall::EpollCtl => handle_epoll_ctl(args),
        Syscall::EpollWait => handle_epoll_wait(args),
        Syscall::SchedSetScheduler => handle_sched_set_scheduler(args),
        Syscall::SchedGetScheduler => handle_sched_get_scheduler(args),
        Syscall::CgroupCreate => handle_cgroup_create(args),
        Syscall::CgroupAddProcess => handle_cgroup_add_process(args),
        Syscall::CgroupSetCpuMax => handle_cgroup_set_cpu_max(args),
        Syscall::CgroupSetMemoryMax => handle_cgroup_set_memory_max(args),
        Syscall::CgroupSetPidsMax => handle_cgroup_set_pids_max(args),
        Syscall::EventFdCreate => handle_eventfd_create(args),
        Syscall::EventFdRead => handle_eventfd_read(args),
        Syscall::EventFdWrite => handle_eventfd_write(args),
        Syscall::TimerFdCreate => handle_timerfd_create(args),
        Syscall::TimerFdSettime => handle_timerfd_settime(args),
        Syscall::TimerFdGettime => handle_timerfd_gettime(args),
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
    let ctx = crate::security::current_context();
    SyscallResult::Success(ctx.uid as u64)
}

fn handle_getgid(_args: SyscallArgs) -> SyscallResult {
    let ctx = crate::security::current_context();
    SyscallResult::Success(ctx.gid as u64)
}

fn handle_setuid(args: SyscallArgs) -> SyscallResult {
    let new_uid = args.arg0 as u32;
    let current = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(-1),
    };
    let mut inner = current.inner.lock();
    if !inner.sec_ctx.has_effective(crate::security::capabilities::Capability::Setuid) {
        return SyscallResult::Error(-1);
    }
    inner.sec_ctx.uid = new_uid;
    SyscallResult::Success(0)
}

fn handle_setgid(args: SyscallArgs) -> SyscallResult {
    let new_gid = args.arg0 as u32;
    let current = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(-1),
    };
    let mut inner = current.inner.lock();
    if !inner.sec_ctx.has_effective(crate::security::capabilities::Capability::Setgid) {
        return SyscallResult::Error(-1);
    }
    inner.sec_ctx.gid = new_gid;
    SyscallResult::Success(0)
}

fn handle_capget(args: SyscallArgs) -> SyscallResult {
    let header_ptr = args.arg0 as *const turnix_abi::syscall::CapHeader;
    let data_ptr = args.arg1 as *mut turnix_abi::syscall::CapData;
    if header_ptr.is_null() || data_ptr.is_null() {
        return SyscallResult::Error(-1);
    }
    let header = unsafe { core::ptr::read(header_ptr) };
    if header.version != turnix_abi::syscall::LINUX_CAPABILITY_VERSION {
        return SyscallResult::Error(-1);
    }
    let caps = {
        let current = match crate::task::scheduler::get_current_process() {
            Some(p) => p,
            None => return SyscallResult::Error(-1),
        };
        let inner = current.inner.lock();
        inner.sec_ctx.caps
    };
    let data = turnix_abi::syscall::CapData {
        effective: caps.effective,
        permitted: caps.permitted,
        inheritable: caps.inheritable,
    };
    unsafe { core::ptr::write(data_ptr, data) };
    SyscallResult::Success(0)
}

fn handle_capset(args: SyscallArgs) -> SyscallResult {
    let header_ptr = args.arg0 as *const turnix_abi::syscall::CapHeader;
    let data_ptr = args.arg1 as *const turnix_abi::syscall::CapData;
    if header_ptr.is_null() || data_ptr.is_null() {
        return SyscallResult::Error(-1);
    }
    let header = unsafe { core::ptr::read(header_ptr) };
    if header.version != turnix_abi::syscall::LINUX_CAPABILITY_VERSION {
        return SyscallResult::Error(-1);
    }
    let new_data = unsafe { core::ptr::read(data_ptr) };

    let current = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(-1),
    };
    let mut inner = current.inner.lock();

    // LSM capability_check hook
    if crate::security::lsm::check_capability(
        crate::security::capabilities::Capability::Setpcap as u32,
        inner.sec_ctx.uid,
        inner.sec_ctx.gid,
    )
    .is_err()
    {
        return SyscallResult::Error(-1); // EPERM
    }

    // CAP_SETPCAP is required to change capability sets.
    if !inner.sec_ctx.has_capability(crate::security::capabilities::Capability::Setpcap) {
        return SyscallResult::Error(-1); // EPERM
    }

    // Only allow setting bits that are in the permitted set (safe capabilities).
    let allow_mask = inner.sec_ctx.caps.permitted;
    if (new_data.effective | new_data.permitted | new_data.inheritable) & !allow_mask != 0 {
        return SyscallResult::Error(-1); // EPERM - cannot add capabilities not in permitted
    }

    inner.sec_ctx.caps.effective = new_data.effective;
    inner.sec_ctx.caps.permitted = new_data.permitted;
    inner.sec_ctx.caps.inheritable = new_data.inheritable;

    SyscallResult::Success(0)
}

/// Handle prctl syscall — only supports PR_SET_SECCOMP.
fn handle_prctl(args: SyscallArgs) -> SyscallResult {
    let option = args.arg0 as u32;
    let arg1 = args.arg1 as u32;
    let arg2 = args.arg2; // user-space pointer to sock_fprog

    if option != crate::security::seccomp::PR_SET_SECCOMP {
        return SyscallResult::Error(-1); // Unknown option
    }

    if arg1 != crate::security::seccomp::SECCOMP_MODE_FILTER {
        return SyscallResult::Error(-1); // Only FILTER mode supported
    }

    // Read the sock_fprog structure from user space:
    // struct sock_fprog {
    //     unsigned short len;    // number of instructions
    //     struct sock_filter *filter; // pointer to instructions
    // };
    let sock_fprog_ptr = arg2 as *const u8;
    if sock_fprog_ptr.is_null() {
        return SyscallResult::Error(-14); // EFAULT
    }

    // Read len (u16) at offset 0
    let filter_len = unsafe { core::ptr::read_unaligned(sock_fprog_ptr as *const u16) } as usize;
    // Read filter pointer (u64) at offset 2 (with alignment padding on x86_64, typically 8)
    // The sock_fprog struct has: len: u16, padding: [u8; 6], filter: *const sock_filter
    let filter_ptr = unsafe {
        let ptr_ptr = (sock_fprog_ptr as usize + 8) as *const *const crate::security::seccomp::BpfInstruction;
        core::ptr::read(ptr_ptr)
    };

    if filter_len == 0 || filter_len > 4096 || filter_ptr.is_null() {
        return SyscallResult::Error(-22); // EINVAL
    }

    // Read instructions from user space
    let mut instructions = alloc::vec![
        crate::security::seccomp::BpfInstruction {
            code: 0,
            jt: 0,
            jf: 0,
            k: 0
        };
        filter_len
    ];
    for (i, slot) in instructions.iter_mut().enumerate() {
        unsafe {
            let insn_ptr = filter_ptr.add(i);
            *slot = core::ptr::read_unaligned(insn_ptr);
        }
    }

    // Create the filter
    let filter = match crate::security::seccomp::SeccompFilter::new(instructions, true) {
        Some(f) => f,
        None => return SyscallResult::Error(-22), // EINVAL
    };

    // Install the filter on the current process
    let current = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(-1),
    };
    let mut inner = current.inner.lock();

    // If a filter is already installed, reject (cannot change seccomp policy)
    if inner.seccomp_filter.is_some() {
        return SyscallResult::Error(-1); // EPERM
    }

    inner.seccomp_filter = Some(filter);
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
    let tid = match crate::task::scheduler::get_current_task_id() {
        Some(tid) => tid,
        None => return SyscallResult::Error(1),
    };
    let pid = crate::process::ProcessId(tid.as_usize());
    // If we have a PID namespace, translate to ns-local PID.
    if let Some(proc) = crate::task::scheduler::get_current_process() {
        let pcb = proc.inner.lock();
        let ns = pcb.nsproxy.effective_pid_ns();
        if let Some(local) = ns.global_to_local(pid) {
            return SyscallResult::Success(local as u64);
        }
    }
    SyscallResult::Success(tid.as_usize() as u64)
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
    let exit_code = args.arg0 as i32;
    crate::serial::println!("[syscall] exit({})", exit_code);

    if let Some(process) = crate::task::scheduler::get_current_process() {
        let pml4_frame;
        let ppid;
        {
            let mut inner = process.inner.lock();

            // Close every open FD.
            for slot in inner.fd_table.iter_mut() {
                *slot = None;
            }

            // Capture the PML4 frame so we can release the address space.
            pml4_frame = inner.pml4_frame;
            ppid = inner.ppid;

            // Transition to Zombie so the parent can reap us.
            inner.state = crate::process::ProcessState::Zombie { exit_code };

            // Clear VMA metadata (physical frames are freed below).
            inner.vma_set = crate::memory::vma::VmaSet::new();

            // Reparent any children whose parent is about to disappear.
            let my_pid = inner.id;
            let table = crate::process::PROCESS_TABLE.lock();
            for pcb_arc in table.values() {
                let mut pcb = pcb_arc.lock();
                if pcb.ppid == my_pid {
                    crate::serial::println!(
                        "[exit] reparenting PID {:?} → init", pcb.id
                    );
                    pcb.ppid = crate::process::ProcessId(1);
                }
            }
            drop(table);
        } // inner lock released

        // Release the process's address space: free all user page tables
        // and physical frames so they are not leaked while the Zombie PCB
        // lingers until the parent calls wait().
        {
            let mut guard = crate::boot::FRAME_ALLOCATOR.lock();
            if let Some(ref mut frame_allocator) = *guard {
                let phys_mem_offset = crate::boot::get_phys_mem_offset();
                crate::memory::paging::destroy_user_mappings(
                    pml4_frame,
                    frame_allocator,
                    phys_mem_offset,
                );
            }
        }

        // Deliver SIGCHLD to parent and wake any task blocked in wait().
        {
            let table = crate::process::PROCESS_TABLE.lock();
            if let Some(parent_pcb_arc) = table.get(&ppid) {
                let mut parent = parent_pcb_arc.lock();
                parent.pending_signals.insert(17);
            }
        }
        crate::task::scheduler::wake_tasks_waiting_for_parent(ppid);
    }

    // Mark the current kernel task as Zombie and yield so the scheduler
    // can run another task.  This function never returns.
    crate::task::scheduler::exit_current_task();

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

    // Try network socket table first
    if crate::net::socket::sys_close(fd).is_ok() {
        return SyscallResult::Success(0);
    }

    // Fall back to VFS
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
        None => return SyscallResult::Error(2), // File not found (ENOENT)
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

    // Read FileCaps from the executable's security.capability xattr before dropping VFS lock.
    let file_caps = {
        use crate::security::capabilities::FileCaps;
        if let Ok(Some(xattr_data)) = vfs.xattr_get(path, "security.capability") {
            FileCaps::from_bytes(&xattr_data)
        } else {
            None
        }
    };

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
        file_caps,
    ) {
        Ok(_) => (),
        Err(_) => return SyscallResult::Error(8),
    };

    // Record IMA measurement for the executed binary
    crate::security::ima::measure_exec(&elf_data[..read_len], path);

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

/// Clone syscall handler — wraps fork behaviour but passes namespace flags.
pub fn handle_clone_with_frame(frame: &crate::arch::x86_64::syscall_arch::SyscallFrame) -> u64 {
    let flags = frame.rdi; // arg0: clone flags (CLONE_VM, CLONE_NEWPID, etc.)
    let _child_stack = frame.rsi; // arg1: child stack (unused — we use our own stack)

    let parent_process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return !0,
    };

    let mut frame_allocator_guard = crate::boot::get_frame_allocator().lock();
    let frame_allocator = match frame_allocator_guard.as_mut() {
        Some(fa) => fa,
        None => return !0,
    };
    let phys_mem_offset = crate::boot::get_phys_mem_offset();

    let child_process = parent_process.clone_process(flags, frame_allocator, phys_mem_offset);
    let child_pid = child_process.id().0 as u64;

    drop(frame_allocator_guard);

    let (kernel_pml4_frame, _) = x86_64::registers::control::Cr3::read();
    let mut mapper = unsafe {
        let pml4_ptr = (phys_mem_offset + kernel_pml4_frame.start_address().as_u64())
            .as_mut_ptr::<x86_64::structures::paging::PageTable>();
        x86_64::structures::paging::OffsetPageTable::new(&mut *pml4_ptr, phys_mem_offset)
    };

    let mut frame_allocator_guard2 = crate::boot::get_frame_allocator().lock();
    let frame_allocator2 = frame_allocator_guard2.as_mut().unwrap();

    let child_task = crate::task::Task::new_forked_user(
        child_process.clone(),
        frame,
        &mut mapper,
        frame_allocator2,
        phys_mem_offset,
    );

    {
        let mut process_table = crate::process::PROCESS_TABLE.lock();
        process_table.insert(child_process.id(), child_process.inner.clone());
    }

    crate::task::scheduler::add_task(child_task);

    child_pid
}

fn handle_fork(_args: SyscallArgs) -> SyscallResult {
    // This is just a placeholder; the real implementation is in handle_fork_with_frame
    SyscallResult::Error(0)
}

fn handle_clone(_args: SyscallArgs) -> SyscallResult {
    // Placeholder; real implementation is in handle_clone_with_frame
    SyscallResult::Error(0)
}

/// `wait(status_ptr) -> child_pid`
///
/// Waits for *any* child to exit.  If `status_ptr` (arg0) is non-null it
/// receives the exit code as a 32-bit integer.
///
/// Returns:
///  * `child_pid`   on success
///  * `-10` (ECHILD) if the process has no children
///  * `-4`  (EINTR)  if interrupted (future signal support)
fn handle_wait(args: SyscallArgs) -> SyscallResult {
    let status_ptr = args.arg0 as *mut i32;
    handle_wait_impl(/*pid=*/ -1, status_ptr)
}

/// `waitpid(pid, status_ptr, _options) -> child_pid`
///
/// * `pid == -1`  — wait for any child (same as `wait`)
/// * `pid  >  0`  — wait for the specific child PID
fn handle_waitpid(args: SyscallArgs) -> SyscallResult {
    let pid       = args.arg0 as i64;
    let status_ptr = args.arg1 as *mut i32;
    // arg2 = options (WNOHANG etc.) — ignored for now
    handle_wait_impl(pid as i32, status_ptr)
}

/// Common implementation for wait/waitpid.
///
/// `target_pid == -1`  → any child
/// `target_pid  >  0`  → a specific child
fn handle_wait_impl(target_pid: i32, status_ptr: *mut i32) -> SyscallResult {
    let my_pid = match crate::task::scheduler::get_current_process_id() {
        Some(p) => p,
        None    => return SyscallResult::Error(3), // ESRCH – no current process
    };

    // -----------------------------------------------------------------------
    // Scan PROCESS_TABLE for a zombie child that we should reap.
    // -----------------------------------------------------------------------
    loop {
        let reaped = {
            let mut table = crate::process::PROCESS_TABLE.lock();

            // Collect all child PIDs first, then look for zombies.
            let mut found_any_child = false;
            let mut zombie_pid: Option<crate::process::ProcessId> = None;
            let mut zombie_exit_code: i32 = 0;

            for (pid, pcb_arc) in table.iter() {
                let pcb = pcb_arc.lock();
                if pcb.ppid != my_pid {
                    continue;
                }
                // At least one child exists.
                found_any_child = true;

                // Filter by requested PID.
                if target_pid > 0 {
                    let target = crate::process::ProcessId(target_pid as usize);
                    if *pid != target {
                        continue;
                    }
                }

                if let crate::process::ProcessState::Zombie { exit_code } = pcb.state {
                    zombie_pid = Some(*pid);
                    zombie_exit_code = exit_code;
                    break;
                }
            }

            if !found_any_child {
                // ECHILD: no children at all.
                return SyscallResult::Error(10);
            }

            if let Some(zpid) = zombie_pid {
                // Reap: remove zombie from the process table.
                table.remove(&zpid);
                #[cfg(not(test))]
                crate::serial::println!(
                    "[wait] reaped child PID {:?} exit_code={}", zpid, zombie_exit_code
                );
                Some((zpid, zombie_exit_code))
            } else {
                None
            }
        }; // --- PROCESS_TABLE lock released ---

        if let Some((child_pid, exit_code)) = reaped {
            // Write exit status to user buffer if provided.
            if !status_ptr.is_null() {
                // POSIX encodes exit status as (exit_code & 0xff) << 8.
                let encoded = (exit_code & 0xff) << 8;
                unsafe { status_ptr.write(encoded); }
            }
            return SyscallResult::Success(child_pid.0 as u64);
        }

        // No zombie child yet — block and wait to be woken by a child's exit.
        crate::task::scheduler::block_current_waiting_for_child();
        crate::task::scheduler::yield_task();
        // After being woken we loop back and re-scan.
    }
}

fn handle_yielder(args: SyscallArgs) -> SyscallResult {
    if args.arg0 != 0 {
        crate::task::scheduler::yield_task();
    }
    SyscallResult::Success(0)
}

fn handle_mmap_framebuffer_syscall(_args: SyscallArgs) -> SyscallResult {
    let caller_pid = gpu::current_pid();
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

    // Check CAP_SYS_ADMIN
    if let Some(current) = crate::task::scheduler::get_current_process()
        && !current.inner.lock().sec_ctx.has_capability(
            crate::security::capabilities::Capability::SysAdmin
        )
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

/// `pipe(pipefd: *mut [u64; 2]) -> 0 on success`
///
/// Creates a unidirectional data pipe.  `pipefd[0]` receives the read end fd,
/// `pipefd[1]` receives the write end fd.
fn handle_pipe(args: SyscallArgs) -> SyscallResult {
    let pipefd_ptr = args.arg0 as *mut [u64; 2];

    if pipefd_ptr.is_null() {
        return SyscallResult::Error(14); // EFAULT
    }

    let mut vfs = VFS.lock();
    let (read_idx, write_idx) = vfs.create_pipe();

    let pipefds = [read_idx as u64, write_idx as u64];
    unsafe { pipefd_ptr.write(pipefds); }

    SyscallResult::Success(0)
}

/// `socket(domain: i32, type: i32, protocol: i32) -> fd`
fn handle_socket(args: SyscallArgs) -> SyscallResult {
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
fn handle_bind(args: SyscallArgs) -> SyscallResult {
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
            if addr_len < 3 { return SyscallResult::Error(14); }
            let path_slice = unsafe {
                core::slice::from_raw_parts(addr_ptr.add(2), addr_len - 2)
            };
            // Trim trailing nulls
            let path_len = path_slice.iter().position(|&b| b == 0).unwrap_or(path_slice.len());
            let path = core::str::from_utf8(&path_slice[..path_len]).unwrap_or("");
            if path.is_empty() { return SyscallResult::Error(14); }

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
                && !current.inner.lock().sec_ctx.has_capability(
                    crate::security::capabilities::Capability::NetAdmin
                )
            {
                return SyscallResult::Error(1); // EPERM
            }

            if addr_len < 8 { return SyscallResult::Error(14); }

            // sockaddr_in layout: family(2) + port(2) + addr(4) + zero(8)
            let raw_port = unsafe { core::ptr::read_unaligned(addr_ptr.add(2) as *const u16) };
            let raw_addr = unsafe { core::ptr::read_unaligned(addr_ptr.add(4) as *const u32) };

            let port = u16::from_be(raw_port);
            let ip = smoltcp::wire::IpAddress::Ipv4(
                smoltcp::wire::Ipv4Address::from_bytes(&raw_addr.to_be_bytes())
            );

            match crate::net::socket::sys_bind(fd, ip, port) {
                Ok(()) => SyscallResult::Success(0),
                Err(e) => SyscallResult::Error(e),
            }
        }
        _ => SyscallResult::Error(97), // EAFNOSUPPORT
    }
}

/// `listen(sockfd: i32, backlog: i32) -> 0`
fn handle_listen(args: SyscallArgs) -> SyscallResult {
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
fn handle_accept(args: SyscallArgs) -> SyscallResult {
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
fn handle_connect(args: SyscallArgs) -> SyscallResult {
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
            if addr_len < 3 { return SyscallResult::Error(14); }
            let path_slice = unsafe {
                core::slice::from_raw_parts(addr_ptr.add(2), addr_len - 2)
            };
            let path_len = path_slice.iter().position(|&b| b == 0).unwrap_or(path_slice.len());
            let path = core::str::from_utf8(&path_slice[..path_len]).unwrap_or("");
            if path.is_empty() { return SyscallResult::Error(14); }

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
            if addr_len < 8 { return SyscallResult::Error(14); }

            let raw_port = unsafe { core::ptr::read_unaligned(addr_ptr.add(2) as *const u16) };
            let raw_addr = unsafe { core::ptr::read_unaligned(addr_ptr.add(4) as *const u32) };

            let port = u16::from_be(raw_port);
            let ip = smoltcp::wire::IpAddress::Ipv4(
                smoltcp::wire::Ipv4Address::from_bytes(&raw_addr.to_be_bytes())
            );

            match crate::net::socket::sys_connect(fd, ip, port) {
                Ok(()) => SyscallResult::Success(0),
                Err(e) => SyscallResult::Error(e),
            }
        }
        _ => SyscallResult::Error(97), // EAFNOSUPPORT
    }
}

// -----------------------------------------------------------------------
// Signal syscalls
// -----------------------------------------------------------------------

/// `sigaction(sig: i32, new: *const SigAction, old: *mut SigAction) -> 0`
///
/// `SigAction` layout: { handler_or_default: u64, flags: u32, restorer: u64 }
///   - handler_or_default = 0 → SIG_DFL, 1 → SIG_IGN, else → handler address
fn handle_sigaction(args: SyscallArgs) -> SyscallResult {
    let sig = args.arg0 as u8;
    let new_ptr = args.arg1 as *const [u64; 3]; // {action, flags, restorer}
    let old_ptr = args.arg2 as *mut [u64; 3];

    if !(1..=31).contains(&sig) {
        return SyscallResult::Error(22); // EINVAL
    }
    // SIGKILL and SIGSTOP can't be caught or ignored.
    if sig == 9 || sig == 19 {
        return SyscallResult::Error(22); // EINVAL
    }

    let current = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(4), // ESRCH
    };

    let mut inner = current.inner.lock();

    // Return the old action if requested.
    if !old_ptr.is_null() {
        let old_action = &inner.signal_handlers[sig as usize];
        let old_val = match old_action {
            SignalAction::Default => [0u64, 0, 0],
            SignalAction::Ignore => [1u64, 0, 0],
            SignalAction::Handler(addr) => [*addr, 0, 0],
        };
        unsafe { old_ptr.write(old_val); }
    }

    // Set the new action.
    if !new_ptr.is_null() {
        let new_action = unsafe { *new_ptr };
        let action = match new_action[0] {
            0 => SignalAction::Default,
            1 => SignalAction::Ignore,
            addr => SignalAction::Handler(addr),
        };
        inner.signal_handlers[sig as usize] = action;
    }

    SyscallResult::Success(0)
}

/// `sigprocmask(how: i32, new: *const u64, old: *mut u64) -> 0`
///   how: 0 = SIG_BLOCK, 1 = SIG_UNBLOCK, 2 = SIG_SETMASK
fn handle_sigprocmask(args: SyscallArgs) -> SyscallResult {
    let how = args.arg0 as i32;
    let new_ptr = args.arg1 as *const u64;
    let old_ptr = args.arg2 as *mut u64;

    let current = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(4), // ESRCH
    };

    let mut inner = current.inner.lock();

    // Return the old mask if requested.
    if !old_ptr.is_null() {
        unsafe { old_ptr.write(inner.signal_mask.0); }
    }

    if !new_ptr.is_null() {
        let new_mask_val = unsafe { *new_ptr };
        // SIGKILL and SIGSTOP can't be masked.
        let new_mask = new_mask_val & !((1u64 << 9) | (1u64 << 19));
        match how {
            0 => inner.signal_mask.0 |= new_mask,  // SIG_BLOCK
            1 => inner.signal_mask.0 &= !new_mask, // SIG_UNBLOCK
            2 => inner.signal_mask.0 = new_mask,   // SIG_SETMASK
            _ => return SyscallResult::Error(22),  // EINVAL
        }
    }

    SyscallResult::Success(0)
}

/// `kill(pid: i32, sig: i32) -> 0`
fn handle_kill(args: SyscallArgs) -> SyscallResult {
    let pid = args.arg0 as isize;
    let sig = args.arg1 as u8;

    if sig > 31 {
        return SyscallResult::Error(22); // EINVAL
    }

    let target_pid = if pid <= 0 {
        // pid <= 0 not fully supported; send to current process group.
        match crate::task::scheduler::get_current_process() {
            Some(p) => p.inner.lock().id,
            None => return SyscallResult::Error(3), // ESRCH
        }
    } else {
        crate::process::ProcessId(pid as usize)
    };

    // Check CAP_KILL for cross-user signals
    if target_pid != crate::task::scheduler::get_current_process_id().unwrap_or_default()
        && let Some(current) = crate::task::scheduler::get_current_process()
        && !current.inner.lock().sec_ctx.has_capability(
            crate::security::capabilities::Capability::Kill
        )
    {
        return SyscallResult::Error(1); // EPERM
    }

    if crate::task::signals::send_signal(target_pid, sig) {
        SyscallResult::Success(0)
    } else {
        SyscallResult::Error(3) // ESRCH
    }
}

/// `dup(oldfd: i32) -> newfd`
fn handle_dup(args: SyscallArgs) -> SyscallResult {
    let oldfd = args.arg0 as usize;
    let mut vfs = VFS.lock();
    match vfs.dup_fd(oldfd) {
        Some(newfd) => SyscallResult::Success(newfd as u64),
        None => SyscallResult::Error(9), // EBADF
    }
}

/// `dup2(oldfd: i32, newfd: i32) -> newfd`
fn handle_dup2(args: SyscallArgs) -> SyscallResult {
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

/// `shutdown() -> !`
/// Powers off the machine.  Never returns.
fn handle_shutdown(_args: SyscallArgs) -> SyscallResult {
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
fn handle_read_shutdown_signal(_args: SyscallArgs) -> SyscallResult {
    let pending = crate::acpi::take_init_shutdown_signal();
    SyscallResult::Success(if pending { 1 } else { 0 })
}

fn handle_input_read(args: SyscallArgs) -> SyscallResult {
    // arg0: destination buffer pointer
    // arg1: number of InputEvent slots (sizeof(InputEvent) = 8 bytes each)
    let buf_ptr = args.arg0 as *mut InputEvent;
    let slots = args.arg1 as usize;

    if buf_ptr.is_null() || slots == 0 {
        return SyscallResult::Success(0);
    }

    let mut count: u64 = 0;
    for i in 0..slots {
        if let Some(ev) = crate::input::read_event() {
            unsafe { buf_ptr.add(i).write_unaligned(ev) };
            count += 1;
        } else {
            break;
        }
    }

    SyscallResult::Success(count)
}

fn handle_gbm_create(args: SyscallArgs) -> SyscallResult {
    let caller_pid = gpu::current_pid();
    {
        let mgr = gpu::DRM_MANAGER.lock();
        if !mgr.is_compositor(caller_pid) {
            return SyscallResult::Error(1);
        }
    }

    let width = args.arg0 as u32;
    let height = args.arg1 as u32;
    let format = args.arg2 as u32;

    if width == 0 || height == 0 {
        return SyscallResult::Error(22);
    }

    match gpu::gbm::gbm_create(width, height, format) {
        Some(id) => SyscallResult::Success(id),
        None => SyscallResult::Error(12),
    }
}

fn handle_gbm_map(args: SyscallArgs) -> SyscallResult {
    let caller_pid = gpu::current_pid();
    {
        let mgr = gpu::DRM_MANAGER.lock();
        if !mgr.is_compositor(caller_pid) {
            return SyscallResult::Error(1);
        }
    }

    let id = args.arg0;
    match gpu::gbm::gbm_map(id) {
        Some(addr) => SyscallResult::Success(addr),
        None => SyscallResult::Error(2),
    }
}

fn handle_gbm_destroy(args: SyscallArgs) -> SyscallResult {
    let caller_pid = gpu::current_pid();
    {
        let mgr = gpu::DRM_MANAGER.lock();
        if !mgr.is_compositor(caller_pid) {
            return SyscallResult::Error(1);
        }
    }

    let id = args.arg0;
    gpu::gbm::gbm_destroy(id);
    SyscallResult::Success(0)
}

fn handle_drm_page_flip(args: SyscallArgs) -> SyscallResult {
    let caller_pid = gpu::current_pid();
    let crtc_id = args.arg1 as u32;
    let gbm_id = args.arg0;

    // Gather framebuffer info under DRM_MANAGER lock, then drop it before
    // touching GBM to maintain consistent lock ordering (DRM → GBM).
    let fb_addr;
    let fb_size;
    {
        let mgr = gpu::DRM_MANAGER.lock();
        if !mgr.is_compositor(caller_pid) {
            return SyscallResult::Error(1);
        }
        fb_addr = mgr.framebuffer_addr();
        fb_size = mgr.framebuffer_size();
    }
    if fb_addr == 0 || fb_size == 0 {
        return SyscallResult::Error(2);
    }

    let gbm_phys = match gpu::gbm::gbm_map(gbm_id) {
        Some(addr) => addr,
        None => return SyscallResult::Error(2),
    };
    let buf_size = gpu::gbm::gbm_buffer_size(gbm_id).unwrap_or(fb_size);

    let phys_mem_offset = crate::boot::get_phys_mem_offset();
    let src = (phys_mem_offset + gbm_phys).as_ptr::<u8>();
    let dst = (phys_mem_offset + fb_addr).as_mut_ptr::<u8>();
    let copy_size = core::cmp::min(fb_size, buf_size);

    unsafe {
        core::ptr::copy_nonoverlapping(src, dst, copy_size as usize);
    }

    let mut mgr = gpu::DRM_MANAGER.lock();
    match mgr.page_flip(crtc_id, 0) {
        Ok(()) => SyscallResult::Success(0),
        Err(_) => SyscallResult::Error(5),
    }
}

fn handle_chdir(_args: SyscallArgs) -> SyscallResult {
    todo!("chdir syscall")
}

fn handle_xattr_get(args: SyscallArgs) -> SyscallResult {
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

fn handle_xattr_set(args: SyscallArgs) -> SyscallResult {
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
        && !current.inner.lock().sec_ctx.has_capability(
            crate::security::capabilities::Capability::Setfcap,
        )
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

fn handle_dmesg(args: SyscallArgs) -> SyscallResult {
    let buf_ptr = args.arg0 as *mut u8;
    let buf_size = args.arg1 as usize;

    if buf_ptr.is_null() {
        return SyscallResult::Error(14); // EFAULT
    }

    let mut written = 0;
    let mut output = alloc::vec::Vec::new();

    while let Some(entry) = crate::log_ring::kernel_log_read() {
        let msg = entry.message();
        let line = alloc::format!("[{}] {}\n", entry.level, msg);
        if written + line.len() > buf_size {
            break;
        }
        output.extend_from_slice(line.as_bytes());
        written += line.len();
    }

    if written > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(output.as_ptr(), buf_ptr, written);
        }
    }

    SyscallResult::Success(written as u64)
}

/// `NetSetAddr(iface_id, addr_ptr, netmask_ptr, gateway_ptr) -> 0`
///
/// Sets the IP address, netmask, and gateway for a network interface.
/// - arg0: interface ID (0..7)
/// - arg1: pointer to 4-byte IPv4 address
/// - arg2: pointer to 4-byte netmask
/// - arg3: pointer to 4-byte gateway
fn handle_net_set_addr(args: SyscallArgs) -> SyscallResult {
    // Check CAP_NET_ADMIN
    if let Some(current) = crate::task::scheduler::get_current_process()
        && !current.inner.lock().sec_ctx.has_capability(
            crate::security::capabilities::Capability::NetAdmin,
        )
    {
        return SyscallResult::Error(1); // EPERM
    }

    let iface_id = args.arg0 as u32;
    let addr_ptr = args.arg1 as *const [u8; 4];
    let netmask_ptr = args.arg2 as *const [u8; 4];
    let gateway_ptr = args.arg3 as *const [u8; 4];

    if iface_id >= 8
        || addr_ptr.is_null()
        || netmask_ptr.is_null()
        || gateway_ptr.is_null()
    {
        return SyscallResult::Error(22); // EINVAL
    }

    let addr = unsafe { core::ptr::read_unaligned(addr_ptr) };
    let netmask = unsafe { core::ptr::read_unaligned(netmask_ptr) };
    let gateway = unsafe { core::ptr::read_unaligned(gateway_ptr) };

    // Apply to smoltcp interface as well
    {
        let mut stack = crate::net::smoltcp_iface::NET_STACK.lock();
        let prefix_len = netmask.iter().fold(0u8, |acc, b| acc + b.count_ones() as u8);
        let ip_cidr = smoltcp::wire::IpCidr::new(
            smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::from_bytes(&addr)),
            prefix_len,
        );
        stack.interface.update_ip_addrs(|addrs| {
            addrs.clear();
            let _ = addrs.push(ip_cidr);
        });
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
        addr[0], addr[1], addr[2], addr[3],
        netmask[0], netmask[1], netmask[2], netmask[3],
        gateway[0], gateway[1], gateway[2], gateway[3],
    );

    SyscallResult::Success(0)
}

/// `NetSetRoute(gateway_ptr) -> 0`
///
/// Sets the default gateway on interface 0.
/// - arg0: pointer to 4-byte gateway address
fn handle_net_set_route(args: SyscallArgs) -> SyscallResult {
    // Check CAP_NET_ADMIN
    if let Some(current) = crate::task::scheduler::get_current_process()
        && !current.inner.lock().sec_ctx.has_capability(
            crate::security::capabilities::Capability::NetAdmin,
        )
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
        gateway[0], gateway[1], gateway[2], gateway[3],
    );

    SyscallResult::Success(0)
}

/// `NetQuery(iface_id, resp_ptr) -> 0`
///
/// Queries the current configuration of a network interface.
/// - arg0: interface ID (0..7)
/// - arg1: pointer to NetQueryResp (16 bytes)
fn handle_net_query(args: SyscallArgs) -> SyscallResult {
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

// ---------------------------------------------------------------------------
// POSIX Shared Memory syscalls
// ---------------------------------------------------------------------------

fn handle_ftruncate(args: SyscallArgs) -> SyscallResult {
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

fn handle_mmap2(args: SyscallArgs) -> SyscallResult {
    let addr_hint = args.arg0;
    let length = args.arg1;
    let prot_bits = args.arg2 as u8;
    let flags_bits = args.arg3 as u8;
    let fd = args.arg4 as isize;
    let offset = args.arg5;

    if length == 0 || length > 0x1000_0000 {
        return SyscallResult::Error(22);
    }

    let prot = VmaProt::from_bits_truncate(prot_bits);
    if crate::memory::demand::check_wx(prot) {
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

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let addr = if flags.contains(VmaFlags::MAP_FIXED) {
        Some(VirtAddr::new(addr_hint))
    } else {
        None
    };

    // Anonymous mapping (fd == -1)
    if fd == -1 {
        match process.mmap_anon(addr, length, prot, flags) {
            Ok(start) => SyscallResult::Success(start.as_u64()),
            Err(_) => SyscallResult::Error(12), // ENOMEM
        }
    } else {
        // File-backed mapping
        let fd_usize = fd as usize;
        let (inode, _fd_offset) = {
            let inner = process.inner.lock();
            let fd_entry = match inner.fd_table.get(fd_usize).and_then(|e| e.as_ref()) {
                Some(e) => e.clone(),
                None => return SyscallResult::Error(9), // EBADF
            };
            (fd_entry.inode, fd_entry.offset.load(core::sync::atomic::Ordering::Relaxed))
        };

        let page_aligned_len = length.max(4096).next_multiple_of(4096);
        let vma = crate::memory::vma::Vma {
            start: match addr {
                Some(a) => a,
                None => {
                    let mut inner = process.inner.lock();
                    let a = inner.mmap_next_addr;
                    inner.mmap_next_addr = VirtAddr::new(
                        inner.mmap_next_addr.as_u64().saturating_add(page_aligned_len),
                    );
                    a
                }
            },
            end: VirtAddr::new(0), // set below
            prot,
            backing: crate::memory::vma::VmaBacking::FileBacked { inode: crate::memory::vma::InodeId(inode.0), offset },
            flags,
        };
        let start = vma.start;
        let vma = crate::memory::vma::Vma {
            end: VirtAddr::new(start.as_u64().saturating_add(page_aligned_len)),
            ..vma
        };

        let mut inner = process.inner.lock();
        match inner.vma_set.insert(vma) {
            Ok(()) => SyscallResult::Success(start.as_u64()),
            Err(_) => SyscallResult::Error(12), // ENOMEM
        }
    }
}

fn handle_shm_open(args: SyscallArgs) -> SyscallResult {
    let name_ptr = args.arg0 as *const u8;
    let name_len = args.arg1 as usize;
    let flags = args.arg2 as i32;
    let _mode = args.arg3;

    if name_ptr.is_null() || name_len == 0 {
        return SyscallResult::Error(22); // EINVAL
    }

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

fn handle_shm_unlink(args: SyscallArgs) -> SyscallResult {
    let name_ptr = args.arg0 as *const u8;
    let name_len = args.arg1 as usize;

    if name_ptr.is_null() || name_len == 0 {
        return SyscallResult::Error(22);
    }

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

fn handle_mq_open(args: SyscallArgs) -> SyscallResult {
    let name_ptr = args.arg0 as *const u8;
    let name_len = args.arg1 as usize;
    let flags = args.arg2 as i32;
    let mode = args.arg3 as u32;
    let max_msgs = args.arg4 as usize;
    let max_msg_size = args.arg5 as usize;

    if name_ptr.is_null() || name_len == 0 {
        return SyscallResult::Error(22);
    }

    let name_slice = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let name = match core::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return SyscallResult::Error(22),
    };

    if name.contains('/') {
        return SyscallResult::Error(22);
    }

    let max_msgs = if max_msgs == 0 { crate::ipc::mqueue::MQ_MAX_MSG } else { max_msgs };
    let max_msg_size = if max_msg_size == 0 { crate::ipc::mqueue::MQ_MSG_SIZE } else { max_msg_size };

    let mq =     match crate::ipc::mqueue::mq_open(name, flags, mode, max_msgs, max_msg_size) {
        Ok(q) => q,
        Err(e) => return SyscallResult::Error(e as i64),
    };

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let mut inner = process.inner.lock();
    let fd = inner.fd_table.iter().position(|s| s.is_none()).unwrap_or(inner.fd_table.len());
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

fn handle_mq_close(args: SyscallArgs) -> SyscallResult {
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

fn handle_mq_unlink(args: SyscallArgs) -> SyscallResult {
    let name_ptr = args.arg0 as *const u8;
    let name_len = args.arg1 as usize;

    if name_ptr.is_null() || name_len == 0 {
        return SyscallResult::Error(22);
    }

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

fn handle_mq_send(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let msg_ptr = args.arg1 as *const u8;
    let msg_len = args.arg2 as usize;
    let prio = args.arg3 as u32;

    if msg_ptr.is_null() {
        return SyscallResult::Error(14); // EFAULT
    }

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

fn handle_mq_receive(args: SyscallArgs) -> SyscallResult {
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
            unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), buf_ptr, n); }
            SyscallResult::Success(n as u64)
        }
        Err(e) => SyscallResult::Error(e as i64),
    }
}

// ---------------------------------------------------------------------------
// Epoll syscalls
// ---------------------------------------------------------------------------

fn handle_epoll_create(_args: SyscallArgs) -> SyscallResult {
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
    let fd = inner.fd_table.iter().position(|s| s.is_none()).unwrap_or(inner.fd_table.len());
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

fn handle_epoll_ctl(args: SyscallArgs) -> SyscallResult {
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

fn handle_epoll_wait(args: SyscallArgs) -> SyscallResult {
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
// cgroups v2 syscalls
// ---------------------------------------------------------------------------

fn handle_cgroup_create(args: SyscallArgs) -> SyscallResult {
    let parent_ptr = args.arg0 as *const u8;
    let parent_len = args.arg1 as usize;
    let name_ptr = args.arg2 as *const u8;
    let name_len = args.arg3 as usize;

    if parent_ptr.is_null() || parent_len == 0 || name_ptr.is_null() || name_len == 0 {
        return SyscallResult::Error(22);
    }

    let parent_slice = unsafe { core::slice::from_raw_parts(parent_ptr, parent_len) };
    let parent_path = match core::str::from_utf8(parent_slice) {
        Ok(s) => s,
        Err(_) => return SyscallResult::Error(22),
    };
    let name_slice = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let name = match core::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return SyscallResult::Error(22),
    };

    match crate::cgroup::cgroup_create(parent_path, name) {
        Ok(()) => SyscallResult::Success(0),
        Err(e) => SyscallResult::Error(e as i64),
    }
}

fn handle_cgroup_add_process(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let pid = args.arg2 as u32;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(22);
    }

    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let path = match core::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => return SyscallResult::Error(22),
    };

    match crate::cgroup::cgroup_add_process(path, pid) {
        Ok(()) => SyscallResult::Success(0),
        Err(e) => SyscallResult::Error(e as i64),
    }
}

fn handle_cgroup_set_cpu_max(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let max = args.arg2;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(22);
    }

    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let path = match core::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => return SyscallResult::Error(22),
    };

    match crate::cgroup::cgroup_set_cpu_max(path, max) {
        Ok(()) => SyscallResult::Success(0),
        Err(e) => SyscallResult::Error(e as i64),
    }
}

fn handle_cgroup_set_memory_max(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let max = args.arg2;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(22);
    }

    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let path = match core::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => return SyscallResult::Error(22),
    };

    match crate::cgroup::cgroup_set_memory_max(path, max) {
        Ok(()) => SyscallResult::Success(0),
        Err(e) => SyscallResult::Error(e as i64),
    }
}

fn handle_cgroup_set_pids_max(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let max = args.arg2 as u32;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(22);
    }

    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let path = match core::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => return SyscallResult::Error(22),
    };

    match crate::cgroup::cgroup_set_pids_max(path, max) {
        Ok(()) => SyscallResult::Success(0),
        Err(e) => SyscallResult::Error(e as i64),
    }
}

// ---------------------------------------------------------------------------
// Scheduler class syscalls
// ---------------------------------------------------------------------------

fn handle_sched_set_scheduler(args: SyscallArgs) -> SyscallResult {
    let policy = args.arg0 as u8;
    let priority = args.arg1 as u8;

    let sched_policy = match crate::task::scheduler_class::SchedulingPolicy::from_u8(policy) {
        Some(p) => p,
        None => return SyscallResult::Error(22), // EINVAL
    };

    let actual_priority = if priority == 0 {
        crate::task::scheduler_class::base_priority(sched_policy)
    } else {
        priority
    };

    crate::task::scheduler::set_current_policy(sched_policy, actual_priority);
    SyscallResult::Success(0)
}

fn handle_sched_get_scheduler(_args: SyscallArgs) -> SyscallResult {
    match crate::task::scheduler::get_current_policy() {
        Some((policy, priority)) => SyscallResult::Success(((priority as u64) << 8) | (policy as u8 as u64)),
        None => SyscallResult::Error(1),
    }
}

// ---------------------------------------------------------------------------
// Futex syscall
// ---------------------------------------------------------------------------

fn handle_futex(args: SyscallArgs) -> SyscallResult {
    let uaddr = args.arg0;
    let op = args.arg1 as u32;
    let val = args.arg2 as u32;

    match op {
        crate::ipc::futex::FUTEX_WAIT => {
            match crate::ipc::futex::futex_wait(uaddr, val) {
                Ok(()) => SyscallResult::Success(0),
                Err(e) => SyscallResult::Error(e as i64),
            }
        }
        crate::ipc::futex::FUTEX_WAKE => {
            match crate::ipc::futex::futex_wake(uaddr, val as usize) {
                Ok(n) => SyscallResult::Success(n as u64),
                Err(e) => SyscallResult::Error(e as i64),
            }
        }
        _ => SyscallResult::Error(22), // EINVAL
    }
}

fn helper_alloc_fd(process: &crate::process::Process, fd_entry: crate::vfs::FileDescriptor) -> usize {
    let mut inner = process.inner.lock();
    let fd = inner.fd_table.iter().position(|s| s.is_none()).unwrap_or(inner.fd_table.len());
    if fd >= inner.fd_table.len() {
        inner.fd_table.resize_with(fd + 1, || None);
    }
    inner.fd_table[fd] = Some(fd_entry);
    fd
}

fn handle_eventfd_create(args: SyscallArgs) -> SyscallResult {
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

fn handle_eventfd_read(args: SyscallArgs) -> SyscallResult {
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

fn handle_eventfd_write(args: SyscallArgs) -> SyscallResult {
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

fn handle_timerfd_create(_args: SyscallArgs) -> SyscallResult {
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

fn handle_timerfd_settime(args: SyscallArgs) -> SyscallResult {
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

fn handle_timerfd_gettime(args: SyscallArgs) -> SyscallResult {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{Process, ProcessControlBlock, ProcessId, ProcessState, SignalSet, SignalAction};
    use crate::task::{Task, TaskId, TaskState};
    use crate::memory::vma::VmaSet;
    use spin::Mutex;
    use alloc::sync::Arc;
    use x86_64::{PhysAddr, VirtAddr};
    use x86_64::structures::paging::PhysFrame;
    use turnix_abi::syscall::SyscallArgs;

    struct SafeBootInfo(turnix_abi::boot::BootInfo);
    unsafe impl Sync for SafeBootInfo {}

    static DUMMY_BOOT_INFO: SafeBootInfo = SafeBootInfo(turnix_abi::boot::BootInfo::uefi(turnix_abi::version::ABI_VERSION));

    fn setup_dummy_process() {
        let process = Process {
            inner: Arc::new(Mutex::new(ProcessControlBlock {
                id: ProcessId(1),
                ppid: ProcessId(0),
                state: ProcessState::Running,
                pml4_frame: PhysFrame::containing_address(PhysAddr::new(0)),
                entry_point: VirtAddr::zero(),
                stack_top: VirtAddr::zero(),
                threads: alloc::vec![],
                vma_set: VmaSet::new(),
                mmap_next_addr: VirtAddr::zero(),
                aslr_base: VirtAddr::zero(),
                fd_table: alloc::vec![None; 1024],
                signal_mask: SignalSet::empty(),
                signal_handlers: [SignalAction::Default; 64],
                pending_signals: SignalSet::empty(),
                pending_signal_frame: None,
                sec_ctx: crate::security::SecurityContext::root(),
                nsproxy: crate::security::namespaces::NsProxy::new(),
                seccomp_filter: None,
                cgroup_path: None,
            }))
        };
        let task = Task::new_test(TaskId::new(), process, TaskState::Running);
        crate::task::scheduler::set_current_task_for_test(task);
    }

    fn cleanup() {
        *crate::boot::FRAME_ALLOCATOR.lock() = None;
        *crate::boot::PHYS_MEM_OFFSET.lock() = None;
        *crate::vfs::VFS.lock() = crate::vfs::Vfs::new();
    }

    #[test]
    fn test_exec_nonexistent_path_returns_enoent() {
        setup_dummy_process();

        // Ensure VFS has a mounted root but no such file
        let mut vfs = crate::vfs::VFS.lock();
        *vfs = crate::vfs::Vfs::new();
        vfs.mount("/", Arc::new(crate::fs::tmpfs::TmpfsBackend::new()), crate::fs::vfs::MountFlags::default()).unwrap();
        drop(vfs);

        let path = "/nonexistent_file";
        let args = SyscallArgs::new(
            path.as_ptr() as u64,
            path.len() as u64,
            0,
            0,
        );

        let result = handle_exec(args);
        match result {
            SyscallResult::Error(err) => {
                assert_eq!(err, 2, "Expected ENOENT (2) when file is not found");
            }
            other => panic!("Expected SyscallResult::Error, got {:?}", other),
        }

        // Verify the process remains unchanged (still has empty VMA set, etc.)
        let current_process = crate::task::scheduler::get_current_process().unwrap();
        let inner = current_process.inner.lock();
        assert_eq!(inner.id.0, 1);
        assert!(inner.vma_set.iter().next().is_none());

        cleanup();
    }

    #[test]
    fn test_exec_invalid_elf_magic_returns_enoexec() {
        setup_dummy_process();

        // Set up frame allocator and phys mem offset
        let boot_info = &DUMMY_BOOT_INFO.0;
        let mut guard = crate::boot::FRAME_ALLOCATOR.lock();
        *guard = Some(crate::memory::FrameAllocator::new(boot_info));
        let mut offset_guard = crate::boot::PHYS_MEM_OFFSET.lock();
        *offset_guard = Some(VirtAddr::zero());
        drop(guard);
        drop(offset_guard);

        // Mount a tmpfs root and create a file with invalid ELF magic directly
        let backend = Arc::new(crate::fs::tmpfs::TmpfsBackend::new());
        let inode = backend.inner.lock().create_file(crate::fs::vfs::InodeId(1), "invalid_elf", 0o777).unwrap();
        backend.write(inode, 0, b"not a valid ELF file").unwrap();

        let mut vfs = crate::vfs::VFS.lock();
        *vfs = crate::vfs::Vfs::new();
        vfs.mount("/", backend, crate::fs::vfs::MountFlags::default()).unwrap();
        drop(vfs);

        let path = "/invalid_elf";
        let args = SyscallArgs::new(
            path.as_ptr() as u64,
            path.len() as u64,
            0,
            0,
        );

        let result = handle_exec(args);
        match result {
            SyscallResult::Error(err) => {
                assert_eq!(err, 8, "Expected ENOEXEC (8) when ELF magic is invalid");
            }
            other => panic!("Expected SyscallResult::Error, got {:?}", other),
        }

        // Verify the process remains unchanged
        let current_process = crate::task::scheduler::get_current_process().unwrap();
        let inner = current_process.inner.lock();
        assert_eq!(inner.id.0, 1);
        assert!(inner.vma_set.iter().next().is_none());

        cleanup();
    }
    // -----------------------------------------------------------------------
    // Task-23: wait / waitpid / zombie-reaping unit tests
    // -----------------------------------------------------------------------

    use proptest::prelude::*;

    /// Build a minimal PCB with a given PID and PPID.
    fn make_pcb(
        pid: usize,
        ppid: usize,
        state: ProcessState,
    ) -> Arc<Mutex<ProcessControlBlock>> {
        Arc::new(Mutex::new(ProcessControlBlock {
            id: ProcessId(pid),
            ppid: ProcessId(ppid),
            state,
            pml4_frame: PhysFrame::containing_address(PhysAddr::new(0)),
            entry_point: VirtAddr::zero(),
            stack_top: VirtAddr::zero(),
            threads: alloc::vec![],
            vma_set: VmaSet::new(),
            mmap_next_addr: VirtAddr::zero(),
            aslr_base: VirtAddr::zero(),
            fd_table: alloc::vec![None; 1024],
            signal_mask: SignalSet::empty(),
            signal_handlers: [SignalAction::Default; 64],
            pending_signals: SignalSet::empty(),
            pending_signal_frame: None,
            sec_ctx: crate::security::SecurityContext::root(),
            nsproxy: crate::security::namespaces::NsProxy::new(),
            seccomp_filter: None,
            cgroup_path: None,
        }))
    }

    // Property 15 — Wait Exit Status Round-Trip
    //
    // For any exit code in [0, 255]:
    //   1. The child is in Zombie state with the correct exit code
    //      between exit and wait.
    //   2. `wait` returns exactly that exit code (POSIX-encoded).
    //   3. The zombie child is reaped (removed from PROCESS_TABLE).
    //
    // Validates Requirements 17.3, 17.4.
    proptest! {
        #[test]
        fn test_prop_wait_exit_status_round_trip(exit_code in 0i32..=255) {
            let _guard = crate::test_serial::acquire();
            use crate::process::{ProcessId, ProcessState, PROCESS_TABLE};

            let _parent_pid = ProcessId(900);
            let child_pid  = ProcessId(901);

            // Insert child as Zombie with the generated exit code.
            {
                let mut table = PROCESS_TABLE.lock();
                table.insert(child_pid, make_pcb(901, 900, ProcessState::Zombie { exit_code }));
            }

            // Verify the child is in Zombie state between exit and wait.
            {
                let table = PROCESS_TABLE.lock();
                let child = table.get(&child_pid).unwrap();
                let child_inner = child.lock();
                match child_inner.state {
                    ProcessState::Zombie { exit_code: code } => {
                        assert_eq!(code, exit_code,
                            "child has wrong exit code in Zombie state");
                    }
                    ref other => panic!("expected Zombie, got {:?}", other),
                }
            }

            // Set up current task as parent.
            let parent_proc = crate::process::Process {
                inner: make_pcb(900, 1, ProcessState::Running),
            };
            let task = crate::task::Task::new_test(
                crate::task::TaskId::new(),
                parent_proc,
                crate::task::TaskState::Running,
            );
            crate::task::scheduler::set_current_task_for_test(task);

            // Call wait(-1) and capture the exit status.
            let mut status: i32 = 0xdead;
            let result = handle_wait_impl(-1, &mut status as *mut i32);
            match result {
                SyscallResult::Success(pid) => {
                    assert_eq!(pid, child_pid.0 as u64,
                        "wait returned wrong child PID");
                }
                other => panic!("expected Success, got {:?}", other),
            }

            // POSIX encodes exit status as (exit_code & 0xff) << 8.
            let expected_status = (exit_code & 0xff) << 8;
            assert_eq!(status, expected_status,
                "wait returned wrong exit status for code {}", exit_code);

            // Child must have been reaped from the process table.
            let table = PROCESS_TABLE.lock();
            assert!(
                table.get(&child_pid).is_none(),
                "zombie child should have been reaped from PROCESS_TABLE"
            );
        }
    }

    /// Verify that handle_wait_impl finds a zombie child and returns its PID
    /// and exit code, and that the child is removed from the process table.
    #[test]
    fn test_wait_reaps_zombie_child() {
        let _guard = crate::test_serial::acquire();
        use crate::process::{ProcessId, ProcessState, PROCESS_TABLE};

        let _parent_pid = ProcessId(200);
        let child_pid  = ProcessId(201);
        let exit_code  = 42i32;

        // Insert child (zombie) into process table.
        {
            let mut table = PROCESS_TABLE.lock();
            table.insert(child_pid, make_pcb(201, 200, ProcessState::Zombie { exit_code }));
        }

        // Set up a current task so get_current_process_id() returns parent_pid.
        let parent_proc = Process {
            inner: make_pcb(200, 1, ProcessState::Running),
        };
        let task = Task::new_test(TaskId::new(), parent_proc, TaskState::Running);
        crate::task::scheduler::set_current_task_for_test(task);

        // Call handle_wait_impl — expects Zombie child, should reap it.
        let result = handle_wait_impl(-1, core::ptr::null_mut());
        match result {
            SyscallResult::Success(pid) => {
                assert_eq!(pid, child_pid.0 as u64, "returned wrong child PID");
            }
            other => panic!("expected Success, got {:?}", other),
        }

        // Child must have been removed from the process table.
        let table = PROCESS_TABLE.lock();
        assert!(
            table.get(&child_pid).is_none(),
            "zombie child should have been reaped from PROCESS_TABLE"
        );
    }

    /// Verify that handle_wait_impl returns ECHILD when the current process
    /// has no children at all.
    #[test]
    fn test_wait_returns_echild_when_no_children() {
        let _guard = crate::test_serial::acquire();
        use crate::process::{ProcessId, ProcessState, PROCESS_TABLE};

        // Make sure the process table has no children of PID 300.
        {
            let mut table = PROCESS_TABLE.lock();
            table.retain(|_, pcb| pcb.lock().ppid != ProcessId(300));
        }

        let parent_proc = Process {
            inner: make_pcb(300, 1, ProcessState::Running),
        };
        let task = Task::new_test(TaskId::new(), parent_proc, TaskState::Running);
        crate::task::scheduler::set_current_task_for_test(task);

        let result = handle_wait_impl(-1, core::ptr::null_mut());
        match result {
            SyscallResult::Error(e) => {
                assert_eq!(e, 10, "expected ECHILD (10), got {}", e);
            }
            other => panic!("expected Error(10/ECHILD), got {:?}", other),
        }
    }

    /// Verify that handle_wait_impl for a specific PID returns only that
    /// child's exit code and reaps only that child.
    #[test]
    fn test_waitpid_reaps_specific_child() {
        use crate::process::{ProcessId, ProcessState, PROCESS_TABLE};

        let _parent_pid  = ProcessId(400);
        let child_a_pid = ProcessId(401);
        let child_b_pid = ProcessId(402);

        {
            let mut table = PROCESS_TABLE.lock();
            // child_a: zombie, child_b: running
            table.insert(child_a_pid, make_pcb(401, 400, ProcessState::Zombie { exit_code: 77 }));
            table.insert(child_b_pid, make_pcb(402, 400, ProcessState::Running));
        }

        let parent_proc = Process {
            inner: make_pcb(400, 1, ProcessState::Running),
        };
        let task = Task::new_test(TaskId::new(), parent_proc, TaskState::Running);
        crate::task::scheduler::set_current_task_for_test(task);

        // Wait specifically for child_a.
        let result = handle_wait_impl(401, core::ptr::null_mut());
        match result {
            SyscallResult::Success(pid) => {
                assert_eq!(pid, child_a_pid.0 as u64, "should have reaped child_a");
            }
            other => panic!("expected Success, got {:?}", other),
        }

        // child_a reaped, child_b still present.
        let table = PROCESS_TABLE.lock();
        assert!(table.get(&child_a_pid).is_none(), "child_a should be reaped");
        assert!(table.get(&child_b_pid).is_some(), "child_b should still exist");
        drop(table);

        // Cleanup child_b.
        PROCESS_TABLE.lock().remove(&child_b_pid);
    }

    // ------------------------------------------------------------------
    // Fork / Process-table tests
    // ------------------------------------------------------------------

    #[test]
    fn test_handle_fork_adds_child_to_process_table() {
        let _guard = crate::test_serial::acquire();
        let child_pcb = make_pcb(42, 1, ProcessState::Ready);
        let child_pid = child_pcb.lock().id;

        let mut table = crate::process::PROCESS_TABLE.lock();
        table.insert(child_pid, child_pcb.clone());
        drop(table);

        let table = crate::process::PROCESS_TABLE.lock();
        let found = table.get(&child_pid);
        assert!(found.is_some(), "child should be in process table");
        assert_eq!(found.unwrap().lock().ppid, ProcessId(1));
        drop(table);

        let mut table = crate::process::PROCESS_TABLE.lock();
        table.remove(&child_pid);
    }

    // ------------------------------------------------------------------
    // Dup / Dup2 tests
    // ------------------------------------------------------------------

    fn make_test_fd(vfs: &mut crate::vfs::Vfs, name: &str) -> usize {
        vfs.insert_fd(crate::vfs::FileDescriptor::new(
            crate::vfs::InodeId(0),
            alloc::sync::Arc::new(crate::fs::tmpfs::TmpfsBackend::new()),
            crate::vfs::OpenFlags::RDWR,
            crate::vfs::FdKind::Regular,
            alloc::string::String::from(name),
        ))
    }

    #[test]
    fn dup_returns_new_fd() {
        let mut vfs = crate::vfs::VFS.lock();
        let fd = make_test_fd(&mut vfs, "test");
        let newfd = vfs.dup_fd(fd).expect("dup should succeed");
        assert_ne!(fd, newfd, "dup must return a different fd number");
        assert!(vfs.get_fd(fd).is_some(), "original fd must remain open");
        assert!(vfs.get_fd(newfd).is_some(), "new fd must exist");
    }

    #[test]
    fn dup2_uses_specified_fd() {
        let mut vfs = crate::vfs::VFS.lock();
        let fd = make_test_fd(&mut vfs, "test");
        let target = 99usize;
        let result = vfs.dup2_fd(fd, target).expect("dup2 should succeed");
        assert_eq!(result, target, "dup2 must return the target fd");
        assert!(vfs.get_fd(fd).is_some(), "original fd must remain open");
        assert!(vfs.get_fd(target).is_some(), "target fd must exist");
        let _ = vfs.close_fd(target);
    }

    #[test]
    fn dup2_closes_existing_target() {
        let mut vfs = crate::vfs::VFS.lock();
        let fd_a = make_test_fd(&mut vfs, "a");
        let fd_b = make_test_fd(&mut vfs, "b");
        let result = vfs.dup2_fd(fd_a, fd_b).expect("dup2 should succeed");
        assert_eq!(result, fd_b, "dup2 must return fd_b");
        assert!(vfs.get_fd(fd_b).is_some(), "target fd must still exist");
    }

    #[test]
    fn dup2_same_fd_is_noop() {
        let mut vfs = crate::vfs::VFS.lock();
        let fd = make_test_fd(&mut vfs, "test");
        let result = vfs.dup2_fd(fd, fd).expect("dup2(oldfd, oldfd) should succeed");
        assert_eq!(result, fd, "dup2(oldfd, oldfd) must return oldfd");
        assert!(vfs.get_fd(fd).is_some(), "fd must still exist");
    }

    #[test]
    fn dup_bad_fd_returns_none() {
        let mut vfs = crate::vfs::VFS.lock();
        assert!(vfs.dup_fd(9999).is_none(), "dup of invalid fd must return None");
    }

    #[test]
    fn dup2_bad_fd_returns_none() {
        let mut vfs = crate::vfs::VFS.lock();
        assert!(vfs.dup2_fd(9999, 100).is_none(), "dup2 of invalid oldfd must return None");
    }
}
