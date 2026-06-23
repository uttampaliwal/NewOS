use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum CgroupController {
    cpu,
    memory,
    pids,
    io,
}

pub struct Cgroup {
    pub name: String,
    pub path: String,
    pub parent_path: String,
    pub procs: Vec<u32>,
    pub cpu_max: Option<u64>,
    pub memory_max: Option<u64>,
    pub pids_max: Option<u32>,
    pub controllers: Vec<CgroupController>,
    /// CPU usage: accumulated ticks in current period.
    pub cpu_used: u64,
    /// CPU quota: max ticks per period (same value as cpu_max).
    pub cpu_period_ticks: u64,
    /// Memory usage: current RSS in bytes.
    pub memory_used: u64,
}

static CGROUPS: Mutex<Vec<Cgroup>> = Mutex::new(Vec::new());

/// Global CPU accounting: total ticks consumed across all cgroups this period.
static CPU_TOTAL_TICKS: AtomicU64 = AtomicU64::new(0);

pub const CPU_PERIOD_MS: u64 = 100; // 100ms period

pub fn init() {
    let mut groups = CGROUPS.lock();
    groups.push(Cgroup {
        name: String::from("/"),
        path: String::from("/"),
        parent_path: String::new(),
        procs: Vec::new(),
        cpu_max: None,
        memory_max: None,
        pids_max: None,
        controllers: Vec::new(),
        cpu_used: 0,
        cpu_period_ticks: 0,
        memory_used: 0,
    });
}

fn resolve_path(base: &str, name: &str) -> String {
    if base == "/" {
        alloc::format!("/{}", name)
    } else {
        alloc::format!("{}/{}", base, name)
    }
}

pub fn cgroup_create(parent_path: &str, name: &str) -> Result<(), i32> {
    let path = resolve_path(parent_path, name);
    let mut groups = CGROUPS.lock();
    if groups.iter().any(|g| g.path == path) {
        return Err(17); // EEXIST
    }
    let parent = groups.iter().find(|g| g.path == parent_path).ok_or(2)?;
    let cpu_max = parent.cpu_max;
    let memory_max = parent.memory_max;
    let pids_max = parent.pids_max;
    let controllers = parent.controllers.clone();
    groups.push(Cgroup {
        name: String::from(name),
        path,
        parent_path: String::from(parent_path),
        procs: Vec::new(),
        cpu_max,
        memory_max,
        pids_max,
        controllers,
        cpu_used: 0,
        cpu_period_ticks: 0,
        memory_used: 0,
    });
    Ok(())
}

pub fn cgroup_add_process(path: &str, pid: u32) -> Result<(), i32> {
    let mut groups = CGROUPS.lock();
    let cgroup = groups.iter_mut().find(|g| g.path == path).ok_or(2)?;
    if cgroup.pids_max.is_some_and(|max| cgroup.procs.len() >= max as usize) {
        return Err(28); // ENOSPC
    }
    if !cgroup.procs.contains(&pid) {
        cgroup.procs.push(pid);
    }
    Ok(())
}

pub fn cgroup_set_cpu_max(path: &str, max: u64) -> Result<(), i32> {
    let mut groups = CGROUPS.lock();
    let cgroup = groups.iter_mut().find(|g| g.path == path).ok_or(2)?;
    cgroup.cpu_max = Some(max);
    cgroup.cpu_period_ticks = max;
    Ok(())
}

pub fn cgroup_set_memory_max(path: &str, max: u64) -> Result<(), i32> {
    let mut groups = CGROUPS.lock();
    let cgroup = groups.iter_mut().find(|g| g.path == path).ok_or(2)?;
    cgroup.memory_max = Some(max);
    Ok(())
}

pub fn cgroup_set_pids_max(path: &str, max: u32) -> Result<(), i32> {
    let mut groups = CGROUPS.lock();
    let cgroup = groups.iter_mut().find(|g| g.path == path).ok_or(2)?;
    cgroup.pids_max = Some(max);
    Ok(())
}

/// Record one tick of CPU usage for a process's cgroup.
/// Returns false if the cgroup has exceeded its cpu_max quota.
pub fn cgroup_cpu_tick(pid: u32) -> bool {
    let mut groups = CGROUPS.lock();
    if let Some(cgroup) = groups.iter_mut().find(|g| g.procs.contains(&pid)) {
        cgroup.cpu_used += 1;
        if let Some(max) = cgroup.cpu_max {
            return cgroup.cpu_used <= max;
        }
    }
    true
}

/// Reset all cgroup CPU accounting (called at period boundary).
pub fn cgroup_cpu_reset_period() {
    let mut groups = CGROUPS.lock();
    for g in groups.iter_mut() {
        g.cpu_used = 0;
    }
    CPU_TOTAL_TICKS.store(0, Ordering::Relaxed);
}

/// Record memory allocation for a process's cgroup.
/// Returns false if the cgroup has exceeded its memory_max.
pub fn cgroup_memory_alloc(pid: u32, bytes: u64) -> bool {
    let mut groups = CGROUPS.lock();
    if let Some(cgroup) = groups.iter_mut().find(|g| g.procs.contains(&pid)) {
        cgroup.memory_used += bytes;
        if let Some(max) = cgroup.memory_max {
            return cgroup.memory_used <= max;
        }
    }
    true
}

/// Record memory deallocation for a process's cgroup.
pub fn cgroup_memory_free(pid: u32, bytes: u64) {
    let mut groups = CGROUPS.lock();
    if let Some(cgroup) = groups.iter_mut().find(|g| g.procs.contains(&pid)) {
        cgroup.memory_used = cgroup.memory_used.saturating_sub(bytes);
    }
}

/// Check if a process's cgroup has exceeded its memory limit.
pub fn cgroup_memory_exceeded(pid: u32) -> bool {
    let groups = CGROUPS.lock();
    if let Some(cgroup) = groups.iter().find(|g| g.procs.contains(&pid))
        && let Some(max) = cgroup.memory_max
    {
        return cgroup.memory_used > max;
    }
    false
}

/// Find the cgroup path for a given PID.
pub fn cgroup_find_for_pid(pid: u32) -> Option<String> {
    let groups = CGROUPS.lock();
    groups.iter().find(|g| g.procs.contains(&pid)).map(|g| g.path.clone())
}
