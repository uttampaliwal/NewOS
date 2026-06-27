use super::SyscallResult;
use crate::process::SignalAction;
use turnix_abi::syscall::SyscallArgs;

pub fn handle_getuid(_args: SyscallArgs) -> SyscallResult {
    let ctx = crate::security::current_context();
    SyscallResult::Success(ctx.uid as u64)
}

pub fn handle_getgid(_args: SyscallArgs) -> SyscallResult {
    let ctx = crate::security::current_context();
    SyscallResult::Success(ctx.gid as u64)
}

pub fn handle_setuid(args: SyscallArgs) -> SyscallResult {
    let new_uid = args.arg0 as u32;
    let current = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(-1),
    };
    let mut inner = current.inner.lock();
    if !inner
        .sec_ctx
        .has_effective(crate::security::capabilities::Capability::Setuid)
    {
        return SyscallResult::Error(-1);
    }
    inner.sec_ctx.uid = new_uid;
    SyscallResult::Success(0)
}

pub fn handle_setgid(args: SyscallArgs) -> SyscallResult {
    let new_gid = args.arg0 as u32;
    let current = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return SyscallResult::Error(-1),
    };
    let mut inner = current.inner.lock();
    if !inner
        .sec_ctx
        .has_effective(crate::security::capabilities::Capability::Setgid)
    {
        return SyscallResult::Error(-1);
    }
    inner.sec_ctx.gid = new_gid;
    SyscallResult::Success(0)
}

pub fn handle_capget(args: SyscallArgs) -> SyscallResult {
    let header_ptr = args.arg0 as *const turnix_abi::syscall::CapHeader;
    let data_ptr = args.arg1 as *mut turnix_abi::syscall::CapData;
    if header_ptr.is_null() || data_ptr.is_null() {
        return SyscallResult::Error(-1);
    }
    // Safety: header_ptr is a user-space pointer validated by the null check above.
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
    // Safety: data_ptr is a user-space pointer validated by the null check above.
    unsafe { core::ptr::write(data_ptr, data) };
    SyscallResult::Success(0)
}

pub fn handle_capset(args: SyscallArgs) -> SyscallResult {
    let header_ptr = args.arg0 as *const turnix_abi::syscall::CapHeader;
    let data_ptr = args.arg1 as *const turnix_abi::syscall::CapData;
    if header_ptr.is_null() || data_ptr.is_null() {
        return SyscallResult::Error(-1);
    }
    // Safety: header_ptr is a user-space pointer validated by the null check above.
    let header = unsafe { core::ptr::read(header_ptr) };
    if header.version != turnix_abi::syscall::LINUX_CAPABILITY_VERSION {
        return SyscallResult::Error(-1);
    }
    // Safety: data_ptr is a user-space pointer validated by the null check above.
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
    if !inner
        .sec_ctx
        .has_capability(crate::security::capabilities::Capability::Setpcap)
    {
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
pub fn handle_prctl(args: SyscallArgs) -> SyscallResult {
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
    // Safety: sock_fprog_ptr is a user-space pointer validated by the null check above.
    let filter_len = unsafe { core::ptr::read_unaligned(sock_fprog_ptr as *const u16) } as usize;
    // Read filter pointer (u64) at offset 2 (with alignment padding on x86_64, typically 8)
    // The sock_fprog struct has: len: u16, padding: [u8; 6], filter: *const sock_filter
    // Safety: sock_fprog_ptr is a user-space pointer validated by the null check above.
    let filter_ptr = unsafe {
        let ptr_ptr =
            (sock_fprog_ptr as usize + 8) as *const *const crate::security::seccomp::BpfInstruction;
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
        // Safety: filter_ptr is a user-space pointer, i is bounded by filter_len (<= 4096).
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

pub fn handle_getpid(_args: SyscallArgs) -> SyscallResult {
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

pub fn handle_uptime(_args: SyscallArgs) -> SyscallResult {
    let ticks = crate::task::scheduler::get_uptime_ticks();
    SyscallResult::Success(ticks)
}

pub fn handle_exit(args: SyscallArgs) -> SyscallResult {
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
                    crate::serial::println!("[exit] reparenting PID {:?} → init", pcb.id);
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

pub fn handle_exec(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let argv_ptr = args.arg2 as *const *const u8;
    let envp_ptr = args.arg3 as *const *const u8;

    // Validate path pointer and len
    if path_ptr.is_null() || path_len == 0 || path_len > 4096 {
        return SyscallResult::Error(1);
    }

    // Convert user path to &str
    // Safety: path_ptr is a user-space pointer validated by null/len checks above.
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
            // Safety: argv_ptr is a user-space pointer from syscall args, i is bounded by null terminator.
            let str_ptr = unsafe { *argv_ptr.add(i) };
            if str_ptr.is_null() {
                break;
            }
            // Get string length (find null terminator)
            let mut len = 0;
            // Safety: str_ptr is a user-space pointer read from argv, len bounded by 4096 check.
            while unsafe { *str_ptr.add(len) } != 0 {
                len += 1;
                if len > 4096 {
                    return SyscallResult::Error(2);
                }
            }
            // Safety: str_ptr is a user-space pointer, len is bounded by null terminator search.
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
            // Safety: envp_ptr is a user-space pointer from syscall args, i bounded by null terminator.
            let str_ptr = unsafe { *envp_ptr.add(i) };
            if str_ptr.is_null() {
                break;
            }
            // Get string length (find null terminator)
            let mut len = 0;
            // Safety: str_ptr is a user-space pointer read from envp, len bounded by 4096 check.
            while unsafe { *str_ptr.add(len) } != 0 {
                len += 1;
                if len > 4096 {
                    return SyscallResult::Error(2);
                }
            }
            // Safety: str_ptr is a user-space pointer, len is bounded by null terminator search.
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
    crate::serial::println!("[fork] handle_fork_with_frame entered");
    // 1. Get the current (parent) process
    let parent_process = match crate::task::scheduler::get_current_process() {
        Some(p) => p,
        None => return !0, // error
    };
    crate::serial::println!("[fork] got parent process, locking frame allocator");

    // 2. Get access to frame allocator and physical memory offset
    let mut frame_allocator_guard = crate::boot::get_frame_allocator().lock();
    let frame_allocator = match frame_allocator_guard.as_mut() {
        Some(fa) => fa,
        None => return !0, // error
    };
    let phys_mem_offset = crate::boot::get_phys_mem_offset();

    // 3. Create child process via Process::fork
    crate::serial::println!("[fork] calling parent_process.fork()");
    let child_process = parent_process.fork(frame_allocator, phys_mem_offset);
    let child_pid = child_process.id().0 as u64;
    crate::serial::println!("[fork] fork() returned, child_pid={}", child_pid);

    // 4. Drop frame allocator guard before we try to get the mapper, since it's holding the lock
    drop(frame_allocator_guard);

    // 5. Create a new kernel task for the child process using new_forked_user
    crate::serial::println!("[fork] creating child task...");
    // To get a mapper, we need to get the current kernel page table
    let (kernel_pml4_frame, _) = x86_64::registers::control::Cr3::read();
    // Safety: kernel_pml4_frame is the current kernel page table from Cr3, phys_mem_offset is valid.
    let mut mapper = unsafe {
        let pml4_ptr = (phys_mem_offset + kernel_pml4_frame.start_address().as_u64())
            .as_mut_ptr::<x86_64::structures::paging::PageTable>();
        x86_64::structures::paging::OffsetPageTable::new(&mut *pml4_ptr, phys_mem_offset)
    };

    // Lock frame allocator again
    let mut frame_allocator_guard2 = crate::boot::get_frame_allocator().lock();
    let frame_allocator2 = frame_allocator_guard2.as_mut().unwrap();

    let child_task = match crate::task::Task::new_forked_user(
        child_process.clone(),
        frame,
        &mut mapper,
        frame_allocator2,
        phys_mem_offset,
    ) {
        Ok(task) => task,
        Err(_) => return !0, // ENOMEM
    };

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
    // Safety: kernel_pml4_frame is the current kernel page table from Cr3, phys_mem_offset is valid.
    let mut mapper = unsafe {
        let pml4_ptr = (phys_mem_offset + kernel_pml4_frame.start_address().as_u64())
            .as_mut_ptr::<x86_64::structures::paging::PageTable>();
        x86_64::structures::paging::OffsetPageTable::new(&mut *pml4_ptr, phys_mem_offset)
    };

    let mut frame_allocator_guard2 = crate::boot::get_frame_allocator().lock();
    let frame_allocator2 = frame_allocator_guard2.as_mut().unwrap();

    let child_task = match crate::task::Task::new_forked_user(
        child_process.clone(),
        frame,
        &mut mapper,
        frame_allocator2,
        phys_mem_offset,
    ) {
        Ok(task) => task,
        Err(_) => return !0, // ENOMEM
    };

    {
        let mut process_table = crate::process::PROCESS_TABLE.lock();
        process_table.insert(child_process.id(), child_process.inner.clone());
    }

    crate::task::scheduler::add_task(child_task);

    child_pid
}

pub fn handle_fork(_args: SyscallArgs) -> SyscallResult {
    // This is just a placeholder; the real implementation is in handle_fork_with_frame
    SyscallResult::Error(0)
}

pub fn handle_clone(_args: SyscallArgs) -> SyscallResult {
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
pub fn handle_wait(args: SyscallArgs) -> SyscallResult {
    let status_ptr = args.arg0 as *mut i32;
    handle_wait_impl(/*pid=*/ -1, status_ptr, false)
}

/// `waitpid(pid, status_ptr, options) -> child_pid`
///
/// * `pid == -1`  — wait for any child (same as `wait`)
/// * `pid  >  0`  — wait for the specific child PID
pub fn handle_waitpid(args: SyscallArgs) -> SyscallResult {
    let pid = args.arg0 as i64;
    let status_ptr = args.arg1 as *mut i32;
    let options = args.arg2 as u32;
    let nohang = options & 1 != 0;
    handle_wait_impl(pid as i32, status_ptr, nohang)
}

/// Common implementation for wait/waitpid.
///
/// `target_pid == -1`  → any child
/// `target_pid  >  0`  → a specific child
pub fn handle_wait_impl(target_pid: i32, status_ptr: *mut i32, nohang: bool) -> SyscallResult {
    let my_pid = match crate::task::scheduler::get_current_process_id() {
        Some(p) => p,
        None => return SyscallResult::Error(3), // ESRCH – no current process
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
                    "[wait] reaped child PID {:?} exit_code={}",
                    zpid,
                    zombie_exit_code
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
                // Safety: status_ptr is a user-space pointer from syscall arg, null check ensures non-null.
                unsafe {
                    status_ptr.write(encoded);
                }
            }
            return SyscallResult::Success(child_pid.0 as u64);
        }

        // WNOHANG: return 0 immediately if no zombie child found.
        if nohang {
            return SyscallResult::Success(0);
        }

        // No zombie child yet — block and wait to be woken by a child's exit.
        crate::task::scheduler::block_current_waiting_for_child();
        crate::task::scheduler::yield_task();
        // After being woken we loop back and re-scan.
    }
}

pub fn handle_yielder(args: SyscallArgs) -> SyscallResult {
    if args.arg0 != 0 {
        crate::task::scheduler::yield_task();
    }
    SyscallResult::Success(0)
}

/// `sigaction(sig: i32, new: *const SigAction, old: *mut SigAction) -> 0`
///
/// `SigAction` layout: { handler_or_default: u64, flags: u32, restorer: u64 }
///   - handler_or_default = 0 → SIG_DFL, 1 → SIG_IGN, else → handler address
pub fn handle_sigaction(args: SyscallArgs) -> SyscallResult {
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
        // Safety: old_ptr is a user-space pointer from syscall arg, null check ensures non-null.
        unsafe {
            old_ptr.write(old_val);
        }
    }

    // Set the new action.
    if !new_ptr.is_null() {
        // Safety: new_ptr is a user-space pointer from syscall arg, null check ensures non-null.
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
pub fn handle_sigprocmask(args: SyscallArgs) -> SyscallResult {
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
        // Safety: old_ptr is a user-space pointer from syscall arg, null check ensures non-null.
        unsafe {
            old_ptr.write(inner.signal_mask.0);
        }
    }

    if !new_ptr.is_null() {
        // Safety: new_ptr is a user-space pointer from syscall arg, null check ensures non-null.
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
pub fn handle_kill(args: SyscallArgs) -> SyscallResult {
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
        && !current
            .inner
            .lock()
            .sec_ctx
            .has_capability(crate::security::capabilities::Capability::Kill)
    {
        return SyscallResult::Error(1); // EPERM
    }

    if crate::task::signals::send_signal(target_pid, sig) {
        SyscallResult::Success(0)
    } else {
        SyscallResult::Error(3) // ESRCH
    }
}
