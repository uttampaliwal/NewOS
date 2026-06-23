use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

pub const EPOLLIN: u32 = 0x001;
pub const EPOLLOUT: u32 = 0x004;
pub const EPOLLERR: u32 = 0x008;
pub const EPOLLHUP: u32 = 0x010;
pub const EPOLLRDHUP: u32 = 0x2000;
pub const EPOLLONESHOT: u32 = 0x4000;

pub const EPOLL_CTL_ADD: u32 = 1;
pub const EPOLL_CTL_MOD: u32 = 2;
pub const EPOLL_CTL_DEL: u32 = 3;

#[derive(Clone, Debug)]
pub struct EpollEntry {
    pub fd: usize,
    pub events: u32,
    pub data: u64,
}

#[derive(Clone, Debug)]
pub struct EpollReady {
    pub events: u32,
    pub data: u64,
}

pub struct EpollInstance {
    interests: Mutex<BTreeMap<usize, EpollEntry>>,
    blocked_waiters: Mutex<Vec<crate::task::TaskId>>,
}

impl Default for EpollInstance {
    fn default() -> Self {
        Self::new()
    }
}

impl EpollInstance {
    pub fn new() -> Self {
        EpollInstance {
            interests: Mutex::new(BTreeMap::new()),
            blocked_waiters: Mutex::new(Vec::new()),
        }
    }

    pub fn ctl(&self, op: u32, fd: usize, events: u32, data: u64) -> Result<(), i32> {
        let mut interests = self.interests.lock();
        match op {
            EPOLL_CTL_ADD => {
                if interests.contains_key(&fd) {
                    return Err(17); // EEXIST
                }
                interests.insert(fd, EpollEntry { fd, events, data });
                Ok(())
            }
            EPOLL_CTL_MOD => {
                if let Some(entry) = interests.get_mut(&fd) {
                    entry.events = events;
                    entry.data = data;
                    Ok(())
                } else {
                    Err(2) // ENOENT
                }
            }
            EPOLL_CTL_DEL => {
                if interests.remove(&fd).is_some() {
                    Ok(())
                } else {
                    Err(2) // ENOENT
                }
            }
            _ => Err(22), // EINVAL
        }
    }

    fn poll_events(&self, max_events: usize) -> Vec<EpollReady> {
        let interests = self.interests.lock().clone();
        let mut ready = Vec::new();

        for entry in interests.values() {
            let mut revents = 0u32;

            if let Some(process) = crate::task::scheduler::get_current_process() {
                let inner = process.inner.lock();
                if let Some(fd_entry) = inner.fd_table.get(entry.fd).and_then(|e| e.as_ref()) {
                    match &fd_entry.kind {
                        crate::vfs::FdKind::Pipe(pipe) => {
                            if entry.events & EPOLLIN != 0 && pipe.bytes_available() > 0 {
                                revents |= EPOLLIN;
                            }
                            if entry.events & EPOLLOUT != 0 && pipe.space_available() > 0 {
                                revents |= EPOLLOUT;
                            }
                            if entry.events & EPOLLRDHUP != 0 && !pipe.is_read_end_open() {
                                revents |= EPOLLRDHUP;
                            }
                            if !pipe.is_read_end_open() && !pipe.is_write_end_open() {
                                revents |= EPOLLHUP;
                            }
                        }
                        crate::vfs::FdKind::MessageQueue(_) => {
                            revents |= EPOLLIN | EPOLLOUT;
                        }
                        crate::vfs::FdKind::UnixSocket(socket) => {
                            let (rx_avail, tx_avail) = socket.poll();
                            if entry.events & EPOLLIN != 0 && rx_avail > 0 {
                                revents |= EPOLLIN;
                            }
                            if entry.events & EPOLLOUT != 0 && tx_avail > 0 {
                                revents |= EPOLLOUT;
                            }
                        }
                        _ => {
                            if entry.events & (EPOLLIN | EPOLLOUT) != 0 {
                                revents |= entry.events & (EPOLLIN | EPOLLOUT);
                            }
                        }
                    }
                }
            }

            if revents != 0 {
                ready.push(EpollReady {
                    events: revents | entry.events,
                    data: entry.data,
                });
            }
        }

        ready.truncate(max_events);
        ready
    }

    pub fn wait(&self, max_events: usize, timeout_ms: i32) -> Vec<EpollReady> {
        // Non-blocking poll if timeout is 0.
        if timeout_ms == 0 {
            return self.poll_events(max_events);
        }

        // Try a non-blocking poll first.
        let ready = self.poll_events(max_events);
        if !ready.is_empty() {
            return ready;
        }

        // No events ready — block until woken by notify().
        if let Some(tid) = crate::task::scheduler::get_current_task_id() {
            self.blocked_waiters.lock().push(tid);
            crate::task::scheduler::block_current();
            crate::task::scheduler::yield_task();
        }

        // Re-poll after being woken.
        self.poll_events(max_events)
    }

    /// Wake all tasks blocked in wait() on this epoll instance.
    pub fn notify(&self) {
        let wakers: Vec<crate::task::TaskId> = {
            let mut waiters = self.blocked_waiters.lock();
            core::mem::take(&mut *waiters)
        };
        for tid in wakers {
            crate::task::scheduler::wake_task_by_id(tid);
        }
    }
}

static EPOLL_INSTANCES: Mutex<BTreeMap<usize, alloc::sync::Arc<EpollInstance>>> =
    Mutex::new(BTreeMap::new());

pub fn epoll_create() -> Result<usize, i32> {
    let instance = alloc::sync::Arc::new(EpollInstance::new());
    let mut instances = EPOLL_INSTANCES.lock();
    let id = instances.len();
    instances.insert(id, instance);
    Ok(id)
}

pub fn epoll_get(id: usize) -> Option<alloc::sync::Arc<EpollInstance>> {
    EPOLL_INSTANCES.lock().get(&id).cloned()
}

pub fn epoll_ctl(epfd: usize, op: u32, fd: usize, events: u32, data: u64) -> Result<(), i32> {
    let instances = EPOLL_INSTANCES.lock();
    match instances.get(&epfd) {
        Some(inst) => inst.ctl(op, fd, events, data),
        None => Err(9), // EBADF
    }
}

pub fn epoll_wait(epfd: usize, max_events: usize, timeout_ms: i32) -> Vec<EpollReady> {
    let instances = EPOLL_INSTANCES.lock();
    match instances.get(&epfd) {
        Some(inst) => inst.wait(max_events, timeout_ms),
        None => Vec::new(),
    }
}

/// Wake all epoll instances that are monitoring any fd.
/// Called from pipe/socket/mqueue after state changes.
pub fn notify_all_epoll_waiters() {
    let instances: Vec<alloc::sync::Arc<EpollInstance>> = {
        EPOLL_INSTANCES.lock().values().cloned().collect()
    };
    for inst in instances {
        inst.notify();
    }
}
