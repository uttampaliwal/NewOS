mod fs;
mod process;
mod mm;
mod net;
mod ipc;
mod gpu;
mod sched;
mod misc;
mod io_uring;
#[cfg(test)]
mod tests;

use turnix_abi::syscall::{Syscall, SyscallArgs, SyscallHeader};

pub use process::{handle_fork_with_frame, handle_clone_with_frame};

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

/// Filesystem type constants for the `mount` syscall (arg2).
const FSTYPE_TMPFS: u64 = 0;
const FSTYPE_EXT2: u64 = 1;
const FSTYPE_EXT4: u64 = 2;

fn helper_alloc_fd(process: &crate::process::Process, fd_entry: crate::vfs::FileDescriptor) -> usize {
    let mut inner = process.inner.lock();
    let fd = inner.fd_table.iter().position(|s| s.is_none()).unwrap_or(inner.fd_table.len());
    if fd >= inner.fd_table.len() {
        inner.fd_table.resize_with(fd + 1, || None);
    }
    inner.fd_table[fd] = Some(fd_entry);
    fd
}

pub fn handle_syscall(syscall: Syscall, args: SyscallArgs) -> SyscallResult {
    match syscall {
        Syscall::Write => fs::handle_write(args),
        Syscall::Read => fs::handle_read(args),
        Syscall::Exit => process::handle_exit(args),
        Syscall::Open => fs::handle_open(args),
        Syscall::Close => fs::handle_close(args),
        Syscall::Exec => process::handle_exec(args),
        Syscall::Fork => process::handle_fork(args),
        Syscall::Clone => process::handle_clone(args),
        Syscall::Wait => process::handle_wait(args),
        Syscall::Waitpid => process::handle_waitpid(args),
        Syscall::Yielder => process::handle_yielder(args),
        Syscall::Uptime => process::handle_uptime(args),
        Syscall::Ls => fs::handle_ls(args),
        Syscall::Stat => fs::handle_stat(args),
        Syscall::GetPid => process::handle_getpid(args),
        Syscall::Seek => fs::handle_seek(args),
        Syscall::WriteFile => fs::handle_write_file(args),
        Syscall::GetUid => process::handle_getuid(args),
        Syscall::GetGid => process::handle_getgid(args),
        Syscall::SetUid => process::handle_setuid(args),
        Syscall::SetGid => process::handle_setgid(args),
        Syscall::Brk => mm::handle_brk(args),
        Syscall::Mkdir => fs::handle_mkdir(args),
        Syscall::Unlink => fs::handle_unlink(args),
        Syscall::MmapFramebuffer => mm::handle_mmap_framebuffer_syscall(args),
        Syscall::Mmap => mm::handle_mmap(args),
        Syscall::Munmap => mm::handle_munmap(args),
        Syscall::Mount => fs::handle_mount(args),
        Syscall::Umount => fs::handle_umount(args),
        Syscall::Pipe => fs::handle_pipe(args),
        Syscall::Socket => net::handle_socket(args),
        Syscall::Bind => net::handle_bind(args),
        Syscall::Listen => net::handle_listen(args),
        Syscall::Accept => net::handle_accept(args),
        Syscall::Connect => net::handle_connect(args),
        Syscall::Sigaction => process::handle_sigaction(args),
        Syscall::Sigprocmask => process::handle_sigprocmask(args),
        Syscall::Sigreturn => {
            // Sigreturn is handled specially via handle_sigreturn_with_frame
            // in syscall_dispatch. This arm should not be reached.
            SyscallResult::Success(0)
        }
        Syscall::Kill => process::handle_kill(args),
        Syscall::Dup => fs::handle_dup(args),
        Syscall::Dup2 => fs::handle_dup2(args),
        Syscall::Shutdown => net::handle_shutdown(args),
        Syscall::ReadShutdownSignal => net::handle_read_shutdown_signal(args),
        Syscall::Capget => process::handle_capget(args),
        Syscall::Capset => process::handle_capset(args),
        Syscall::Prctl => process::handle_prctl(args),
        Syscall::InputRead => gpu::handle_input_read(args),
        Syscall::GbmCreate => gpu::handle_gbm_create(args),
        Syscall::GbmMap => gpu::handle_gbm_map(args),
        Syscall::GbmDestroy => gpu::handle_gbm_destroy(args),
        Syscall::DrmPageFlip => gpu::handle_drm_page_flip(args),
        Syscall::Chdir => fs::handle_chdir(args),
        Syscall::Dmesg => misc::handle_dmesg(args),
        Syscall::XattrGet => fs::handle_xattr_get(args),
        Syscall::XattrSet => fs::handle_xattr_set(args),
        Syscall::NetSetAddr => net::handle_net_set_addr(args),
        Syscall::NetSetRoute => net::handle_net_set_route(args),
        Syscall::NetQuery => net::handle_net_query(args),
        Syscall::Ftruncate => fs::handle_ftruncate(args),
        Syscall::Mmap2 => mm::handle_mmap2(args),
        Syscall::ShmOpen => ipc::handle_shm_open(args),
        Syscall::ShmUnlink => ipc::handle_shm_unlink(args),
        Syscall::MqOpen => ipc::handle_mq_open(args),
        Syscall::MqClose => ipc::handle_mq_close(args),
        Syscall::MqUnlink => ipc::handle_mq_unlink(args),
        Syscall::MqSend => ipc::handle_mq_send(args),
        Syscall::MqReceive => ipc::handle_mq_receive(args),
        Syscall::Futex => ipc::handle_futex(args),
        Syscall::EpollCreate => ipc::handle_epoll_create(args),
        Syscall::EpollCtl => ipc::handle_epoll_ctl(args),
        Syscall::EpollWait => ipc::handle_epoll_wait(args),
        Syscall::SchedSetScheduler => sched::handle_sched_set_scheduler(args),
        Syscall::SchedGetScheduler => sched::handle_sched_get_scheduler(args),
        Syscall::CgroupCreate => sched::handle_cgroup_create(args),
        Syscall::CgroupAddProcess => sched::handle_cgroup_add_process(args),
        Syscall::CgroupSetCpuMax => sched::handle_cgroup_set_cpu_max(args),
        Syscall::CgroupSetMemoryMax => sched::handle_cgroup_set_memory_max(args),
        Syscall::CgroupSetPidsMax => sched::handle_cgroup_set_pids_max(args),
        Syscall::EventFdCreate => ipc::handle_eventfd_create(args),
        Syscall::EventFdRead => ipc::handle_eventfd_read(args),
        Syscall::EventFdWrite => ipc::handle_eventfd_write(args),
        Syscall::TimerFdCreate => ipc::handle_timerfd_create(args),
        Syscall::TimerFdSettime => ipc::handle_timerfd_settime(args),
        Syscall::TimerFdGettime => ipc::handle_timerfd_gettime(args),
        Syscall::IoUringSetup => io_uring::handle_io_uring_setup(args),
        Syscall::IoUringEnter => io_uring::handle_io_uring_enter(args),
        Syscall::IoUringRegister => io_uring::handle_io_uring_register(args),
        Syscall::Send => net::handle_send(args),
        Syscall::Recv => net::handle_recv(args),
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
