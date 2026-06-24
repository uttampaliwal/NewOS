use super::SyscallResult;
use turnix_abi::syscall::SyscallArgs;
use crate::memory::vma::{VmaFlags, VmaProt};
use x86_64::VirtAddr;

pub fn handle_brk(args: SyscallArgs) -> SyscallResult {
    let new_brk = args.arg0;

    let process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(1),
    };

    let mut inner = process.inner.lock();

    if new_brk == 0 {
        // Return current mmap_next_addr (break address)
        return SyscallResult::Success(inner.mmap_next_addr.as_u64());
    }

    // Extend the break address
    let current_brk = inner.mmap_next_addr.as_u64();
    if new_brk > current_brk {
        // Growing: allocate anonymous memory from current break to new break
        let length = new_brk - current_brk;
        drop(inner);
        let flags = crate::memory::vma::VmaFlags::MAP_PRIVATE;
        match process.mmap_anon(None, length, crate::memory::vma::VmaProt::READ | crate::memory::vma::VmaProt::WRITE, flags) {
            Ok(_start) => {
                // mmap_anon already advances mmap_next_addr
                SyscallResult::Success(new_brk)
            }
            Err(_) => SyscallResult::Error(12), // ENOMEM
        }
    } else {
        // Shrinking or same: just update the break address
        inner.mmap_next_addr = VirtAddr::new(new_brk);
        SyscallResult::Success(new_brk)
    }
}

pub fn handle_mmap_framebuffer_syscall(_args: SyscallArgs) -> SyscallResult {
    let caller_pid = crate::drivers::gpu::current_pid();
    match crate::drivers::gpu::handle_mmap_framebuffer(caller_pid) {
        Ok(addr) => SyscallResult::Success(addr),
        Err(_) => SyscallResult::Error(-1),
    }
}

pub fn handle_mmap(args: SyscallArgs) -> SyscallResult {
    let addr_hint = args.arg0;
    let length = args.arg1;
    let prot_bits = args.arg2 as u8;
    let flags_bits = args.arg3 as u8;

    if length == 0 || length > 0x1000_0000 {
        return SyscallResult::Error(22);
    }

    // Exactly one of MAP_PRIVATE (bit 0) or MAP_SHARED (bit 1) must be set.
    let has_private = flags_bits & 1 != 0;
    let has_shared = flags_bits & 2 != 0;
    if has_private == has_shared {
        return SyscallResult::Error(22); // EINVAL
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

pub fn handle_munmap(args: SyscallArgs) -> SyscallResult {
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

pub fn handle_mmap2(args: SyscallArgs) -> SyscallResult {
    let addr_hint = args.arg0;
    let length = args.arg1;
    let prot_bits = args.arg2 as u8;
    let flags_bits = args.arg3 as u8;
    let fd = args.arg4 as isize;
    let _offset = args.arg5;

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
        let (inode, fd_offset) = {
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
            backing: crate::memory::vma::VmaBacking::FileBacked { inode: crate::memory::vma::InodeId(inode.0), offset: fd_offset },
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
