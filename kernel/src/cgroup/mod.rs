use alloc::string::String;
use alloc::vec::Vec;
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
}

static CGROUPS: Mutex<Vec<Cgroup>> = Mutex::new(Vec::new());

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
