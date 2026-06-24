use super::SyscallResult;
use turnix_abi::syscall::SyscallArgs;

pub fn handle_dmesg(args: SyscallArgs) -> SyscallResult {
    let buf_ptr = args.arg0 as *mut u8;
    let buf_size = args.arg1 as usize;

    if buf_ptr.is_null() {
        return SyscallResult::Error(14); // EFAULT
    }

    let mut written = 0;
    let mut output = alloc::vec::Vec::new();

    if crate::memory::kasan::is_enabled() {
        let s = crate::memory::kasan::stats();
        let line = alloc::format!(
            "[KASAN] allocs={} frees={} errors={}\n",
            s.allocs,
            s.frees,
            s.errors
        );
        output.extend_from_slice(line.as_bytes());
        written += line.len();
    }

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
