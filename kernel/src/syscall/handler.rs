use crate::elf;
use crate::vfs::VFS;
use newos_abi::syscall::{Syscall, SyscallArgs, SyscallHeader};

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

fn handle_ls(_args: SyscallArgs) -> SyscallResult {
    SyscallResult::Error(1)
}

fn handle_stat(_args: SyscallArgs) -> SyscallResult {
    SyscallResult::Error(1)
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
    crate::serial::print(format_args!("\nexit code: {}\n", code));
    SyscallResult::Success(0)
}

fn handle_open(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(1);
    }
    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let path_str = core::str::from_utf8(path_slice).unwrap_or("");
    let mut vfs = VFS.lock();
    match vfs.open(path_str) {
        Some(fd) => SyscallResult::Success(fd as u64),
        None => SyscallResult::Error(1),
    }
}

fn handle_close(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let mut vfs = VFS.lock();
    if vfs.close(fd) {
        SyscallResult::Success(0)
    } else {
        SyscallResult::Error(1)
    }
}

fn handle_exec(_args: SyscallArgs) -> SyscallResult {
    let elf_data_ptr = _args.arg0 as *const u8;
    let elf_size = _args.arg1 as usize;

    if elf_data_ptr.is_null() || elf_size == 0 {
        crate::serial::print(format_args!("exec: null pointer or zero size\n"));
        return SyscallResult::Error(1);
    }

    let elf_data = unsafe { core::slice::from_raw_parts(elf_data_ptr, elf_size) };

    let header = match elf::parse_header(elf_data) {
        Ok(h) => h,
        Err(e) => {
            crate::serial::print(format_args!("exec: invalid ELF header: {:?}\n", e));
            return SyscallResult::Error(1);
        }
    };

    let mut loaded_segments = 0usize;
    for i in 0..header.program_header_count {
        match elf::parse_program_header(elf_data, header, i) {
            Ok(Some(_ph)) => {
                loaded_segments += 1;
            }
            Ok(None) => {}
            Err(e) => {
                crate::serial::print(format_args!("exec: program header error: {:?}\n", e));
            }
        }
    }

    crate::serial::print(format_args!(
        "exec: loaded {} segments, entry: {:#x}\n",
        loaded_segments, header.entry
    ));

    SyscallResult::Success(header.entry)
}

fn handle_fork(_args: SyscallArgs) -> SyscallResult {
    SyscallResult::Error(1)
}

fn handle_wait(_args: SyscallArgs) -> SyscallResult {
    SyscallResult::Error(1)
}

fn handle_yielder(args: SyscallArgs) -> SyscallResult {
    if args.arg0 != 0 {
        crate::task::scheduler::yield_task();
    }
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
