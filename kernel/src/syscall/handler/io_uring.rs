use super::SyscallResult;
use turnix_abi::syscall::SyscallArgs;

pub fn handle_io_uring_setup(args: SyscallArgs) -> SyscallResult {
    let sq_entries = args.arg0 as u32;

    let pid = match crate::task::scheduler::get_current_process_id() {
        Some(p) => p,
        None => return SyscallResult::Error(4),
    };

    match crate::ipc::io_uring::uring_setup(sq_entries, sq_entries * 2, pid) {
        Ok(fd) => SyscallResult::Success(fd as u64),
        Err(e) => SyscallResult::Error(e as i64),
    }
}

pub fn handle_io_uring_enter(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let to_submit = args.arg1 as u32;
    let min_complete = args.arg2 as u32;
    let flags = args.arg3 as u32;

    match crate::ipc::io_uring::uring_enter(fd, to_submit, min_complete, flags) {
        Ok(n) => SyscallResult::Success(n),
        Err(e) => SyscallResult::Error(e as i64),
    }
}

pub fn handle_io_uring_register(args: SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as usize;
    let opcode = args.arg1 as u32;
    let arg = args.arg2;

    match crate::ipc::io_uring::uring_register(fd, opcode, arg) {
        Ok(n) => SyscallResult::Success(n),
        Err(e) => SyscallResult::Error(e as i64),
    }
}
