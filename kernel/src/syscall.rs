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
    }
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

fn handle_read(_args: SyscallArgs) -> SyscallResult {
    SyscallResult::Error(1)
}

fn handle_exit(args: SyscallArgs) -> SyscallResult {
    let code = args.arg0 as i32;
    crate::serial::print(format_args!("\nexit code: {}\n", code));
    SyscallResult::Success(0)
}

fn handle_open(_args: SyscallArgs) -> SyscallResult {
    SyscallResult::Error(1)
}

fn handle_close(_args: SyscallArgs) -> SyscallResult {
    SyscallResult::Error(1)
}

fn handle_exec(_args: SyscallArgs) -> SyscallResult {
    SyscallResult::Error(1)
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
