use super::SyscallResult;
use turnix_abi::syscall::SyscallArgs;

pub fn handle_sched_set_scheduler(args: SyscallArgs) -> SyscallResult {
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

pub fn handle_sched_get_scheduler(_args: SyscallArgs) -> SyscallResult {
    match crate::task::scheduler::get_current_policy() {
        Some((policy, priority)) => {
            SyscallResult::Success(((priority as u64) << 8) | (policy as u8 as u64))
        }
        None => SyscallResult::Error(1),
    }
}

// ---------------------------------------------------------------------------
// cgroups v2 syscalls
// ---------------------------------------------------------------------------

pub fn handle_cgroup_create(args: SyscallArgs) -> SyscallResult {
    let parent_ptr = args.arg0 as *const u8;
    let parent_len = args.arg1 as usize;
    let name_ptr = args.arg2 as *const u8;
    let name_len = args.arg3 as usize;

    if parent_ptr.is_null() || parent_len == 0 || name_ptr.is_null() || name_len == 0 {
        return SyscallResult::Error(22);
    }

    // Safety: parent_ptr is non-null and parent_len > 0 (checked above); pointer is valid for reads of parent_len bytes.
    let parent_slice = unsafe { core::slice::from_raw_parts(parent_ptr, parent_len) };
    let parent_path = match core::str::from_utf8(parent_slice) {
        Ok(s) => s,
        Err(_) => return SyscallResult::Error(22),
    };
    // Safety: name_ptr is non-null and name_len > 0 (checked above); pointer is valid for reads of name_len bytes.
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

pub fn handle_cgroup_add_process(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let pid = args.arg2 as u32;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(22);
    }

    // Safety: path_ptr is non-null and path_len > 0 (checked above); pointer is valid for reads of path_len bytes.
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

pub fn handle_cgroup_set_cpu_max(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let max = args.arg2;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(22);
    }

    // Safety: path_ptr is non-null and path_len > 0 (checked above); pointer is valid for reads of path_len bytes.
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

pub fn handle_cgroup_set_memory_max(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let max = args.arg2;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(22);
    }

    // Safety: path_ptr is non-null and path_len > 0 (checked above); pointer is valid for reads of path_len bytes.
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

pub fn handle_cgroup_set_pids_max(args: SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let max = args.arg2 as u32;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::Error(22);
    }

    // Safety: path_ptr is non-null and path_len > 0 (checked above); pointer is valid for reads of path_len bytes.
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
