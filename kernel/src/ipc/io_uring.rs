extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use crate::process::ProcessId;

/// io_uring operation codes
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoUringOp {
    Nop = 0,
    Read = 1,
    Write = 2,
    Close = 3,
    Openat = 4,
    Fsync = 5,
    Statx = 6,
    Send = 7,
    Recv = 8,
    PollAdd = 9,
    PollRemove = 10,
    Timeout = 11,
}

/// Submission Queue Entry — submitted by userspace
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Sqe {
    pub op: u8,
    pub flags: u8,
    pub priority: u16,
    pub fd: i32,
    pub off: u64,
    pub addr: u64,
    pub len: u32,
    pub op_flags: u32,
    pub user_data: u64,
    pub buf_index: u16,
    pub buf_group: u16,
    pub _pad: [u64; 2],
}

impl Default for Sqe {
    fn default() -> Self {
        Self {
            op: 0,
            flags: 0,
            priority: 0,
            fd: -1,
            off: 0,
            addr: 0,
            len: 0,
            op_flags: 0,
            user_data: 0,
            buf_index: 0,
            buf_group: 0,
            _pad: [0; 2],
        }
    }
}

/// Completion Queue Entry — written by kernel
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Cqe {
    pub user_data: u64,
    pub res: i32,
    pub flags: u32,
}

/// Ring buffer state for SQ or CQ
#[derive(Debug)]
pub struct RingState {
    pub head: AtomicU32,
    pub tail: AtomicU32,
    pub mask: u32,
    pub entries: u32,
}

impl RingState {
    pub fn new(entries: u32) -> Self {
        Self {
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            mask: entries - 1,
            entries,
        }
    }

    pub fn available(&self) -> u32 {
        self.tail.load(Ordering::Acquire) - self.head.load(Ordering::Acquire)
    }

    pub fn space(&self) -> u32 {
        self.entries - self.available()
    }
}

/// A pending io_uring operation
#[derive(Debug, Clone)]
pub struct PendingOp {
    pub sqe: Sqe,
    pub pid: ProcessId,
}

/// The io_uring instance — one per io_uring_setup() call
pub struct IoUringInstance {
    pub sq_ring: Mutex<RingState>,
    pub cq_ring: Mutex<RingState>,
    pub pending: Mutex<VecDeque<PendingOp>>,
    pub completed: Mutex<VecDeque<Cqe>>,
    pub waiters: Mutex<Vec<crate::task::TaskId>>,
    pub owner: ProcessId,
    pub sq_entries: u32,
    pub cq_entries: u32,
    pub notify_fd: Mutex<Option<usize>>,
}

impl IoUringInstance {
    pub fn new(sq_entries: u32, cq_entries: u32, owner: ProcessId) -> Self {
        Self {
            sq_ring: Mutex::new(RingState::new(sq_entries)),
            cq_ring: Mutex::new(RingState::new(cq_entries)),
            pending: Mutex::new(VecDeque::with_capacity(sq_entries as usize)),
            completed: Mutex::new(VecDeque::with_capacity(cq_entries as usize)),
            waiters: Mutex::new(Vec::new()),
            owner,
            sq_entries,
            cq_entries,
            notify_fd: Mutex::new(None),
        }
    }

    pub fn submit_sqes(&self, sqes: &[Sqe]) -> usize {
        let mut pending = self.pending.lock();
        let mut count = 0;
        for sqe in sqes {
            if sqe.op > IoUringOp::Timeout as u8 {
                continue;
            }
            pending.push_back(PendingOp {
                sqe: *sqe,
                pid: self.owner,
            });
            count += 1;
        }
        count
    }

    pub fn process_completions(&self) -> usize {
        let mut processed = 0;

        let ops: Vec<PendingOp> = {
            let mut pending = self.pending.lock();
            let n = pending.len().min(32);
            pending.drain(..n).collect()
        };

        for op in ops {
            let cqe = self.execute_op(&op.sqe, op.pid);
            let mut completed = self.completed.lock();
            if completed.len() < self.cq_entries as usize {
                completed.push_back(cqe);
                processed += 1;
            }
        }

        if processed > 0 {
            self.wake_waiters();
        }

        processed
    }

    fn execute_op(&self, sqe: &Sqe, pid: ProcessId) -> Cqe {
        let res = match IoUringOp::from(sqe.op) {
            IoUringOp::Nop => 0,
            IoUringOp::Read => self.op_read(sqe, pid),
            IoUringOp::Write => self.op_write(sqe, pid),
            IoUringOp::Close => self.op_close(sqe, pid),
            IoUringOp::Fsync => 0,
            IoUringOp::Openat => -38,
            IoUringOp::Statx => -38,
            IoUringOp::PollAdd => 0,
            IoUringOp::PollRemove => 0,
            IoUringOp::Timeout => 0,
            IoUringOp::Send => self.op_write(sqe, pid),
            IoUringOp::Recv => self.op_read(sqe, pid),
        };

        Cqe {
            user_data: sqe.user_data,
            res,
            flags: 0,
        }
    }

    fn op_read(&self, sqe: &Sqe, pid: ProcessId) -> i32 {
        let target_fd = sqe.fd as usize;
        let buf_addr = sqe.addr as *mut u8;
        let buf_len = sqe.len as usize;

        if buf_addr.is_null() {
            return -14;
        }

        if let Some(process) = crate::task::scheduler::get_current_process() {
            if process.id() != pid {
                return -14;
            }
            let inner = process.inner.lock();
            if let Some(fd_entry) = inner.fd_table.get(target_fd).and_then(|e| e.as_ref()) {
                let kind = fd_entry.kind.clone();
                drop(inner);
                match kind {
                    crate::vfs::FdKind::Pipe(pipe_buf) => {
                        let mut buf = alloc::vec![0u8; buf_len];
                        let n = pipe_buf.read(&mut buf);
                        if n > 0 {
                            unsafe {
                                core::ptr::copy_nonoverlapping(buf.as_ptr(), buf_addr, n);
                            }
                        }
                        n as i32
                    }
                    crate::vfs::FdKind::UnixSocket(sock) => {
                        let mut buf = alloc::vec![0u8; buf_len];
                        let n = sock.read(&mut buf);
                        if n > 0 {
                            unsafe {
                                core::ptr::copy_nonoverlapping(buf.as_ptr(), buf_addr, n);
                            }
                        }
                        n as i32
                    }
                    crate::vfs::FdKind::EventFd(efd) => match efd.read_value() {
                        Ok(val) => {
                            let bytes = val.to_ne_bytes();
                            let len = buf_len.min(8);
                            unsafe {
                                core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_addr, len);
                            }
                            len as i32
                        }
                        Err(_) => -11,
                    },
                    crate::vfs::FdKind::TimerFd(tfd) => match tfd.read_expirations() {
                        Ok(exps) => {
                            let bytes = exps.to_ne_bytes();
                            let len = buf_len.min(8);
                            unsafe {
                                core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_addr, len);
                            }
                            len as i32
                        }
                        Err(_) => -11,
                    },
                    _ => -5,
                }
            } else {
                -9
            }
        } else {
            -14
        }
    }

    fn op_write(&self, sqe: &Sqe, pid: ProcessId) -> i32 {
        let target_fd = sqe.fd as usize;
        let buf_addr = sqe.addr as *const u8;
        let buf_len = sqe.len as usize;

        if buf_addr.is_null() {
            return -14;
        }

        let data = unsafe { alloc::slice::from_raw_parts(buf_addr, buf_len) };

        if let Some(process) = crate::task::scheduler::get_current_process() {
            if process.id() != pid {
                return -14;
            }
            let inner = process.inner.lock();
            if let Some(fd_entry) = inner.fd_table.get(target_fd).and_then(|e| e.as_ref()) {
                let kind = fd_entry.kind.clone();
                drop(inner);
                match kind {
                    crate::vfs::FdKind::Pipe(pipe_buf) => {
                        let n = pipe_buf.write(data);
                        n as i32
                    }
                    crate::vfs::FdKind::UnixSocket(sock) => {
                        let n = sock.write(data);
                        n as i32
                    }
                    crate::vfs::FdKind::EventFd(efd) => {
                        let val = if data.len() >= 8 {
                            let mut bytes = [0u8; 8];
                            bytes.copy_from_slice(&data[..8]);
                            u64::from_ne_bytes(bytes)
                        } else if !data.is_empty() {
                            let mut bytes = [0u8; 8];
                            bytes[..data.len()].copy_from_slice(data);
                            u64::from_ne_bytes(bytes)
                        } else {
                            return -22;
                        };
                        match efd.write_value(val) {
                            Ok(()) => 8,
                            Err(_) => -11,
                        }
                    }
                    _ => -5,
                }
            } else {
                -9
            }
        } else {
            -14
        }
    }

    fn op_close(&self, sqe: &Sqe, pid: ProcessId) -> i32 {
        let target_fd = sqe.fd as usize;

        if let Some(process) = crate::task::scheduler::get_current_process() {
            if process.id() != pid {
                return -14;
            }
            let mut inner = process.inner.lock();
            if target_fd < inner.fd_table.len() && inner.fd_table[target_fd].is_some() {
                inner.fd_table[target_fd] = None;
                0
            } else {
                -9
            }
        } else {
            -14
        }
    }

    fn wake_waiters(&self) {
        let waiters = self.waiters.lock();
        for task_id in waiters.iter() {
            crate::task::scheduler::wake_task_by_id(*task_id);
        }
    }

    pub fn add_waiter(&self, task_id: crate::task::TaskId) {
        self.waiters.lock().push(task_id);
    }

    pub fn remove_waiter(&self, task_id: crate::task::TaskId) {
        self.waiters.lock().retain(|&id| id != task_id);
    }
}

impl From<u8> for IoUringOp {
    fn from(v: u8) -> Self {
        match v {
            0 => IoUringOp::Nop,
            1 => IoUringOp::Read,
            2 => IoUringOp::Write,
            3 => IoUringOp::Close,
            4 => IoUringOp::Openat,
            5 => IoUringOp::Fsync,
            6 => IoUringOp::Statx,
            7 => IoUringOp::Send,
            8 => IoUringOp::Recv,
            9 => IoUringOp::PollAdd,
            10 => IoUringOp::PollRemove,
            11 => IoUringOp::Timeout,
            _ => IoUringOp::Nop,
        }
    }
}

static IO_URING_INSTANCES: Mutex<BTreeMap<usize, Arc<IoUringInstance>>> = Mutex::new(BTreeMap::new());

pub fn uring_setup(sq_entries: u32, cq_entries: u32, owner: ProcessId) -> Result<usize, i32> {
    let sq_entries = sq_entries.clamp(1, 4096);
    let cq_entries = cq_entries.clamp(1, 8192);

    let instance = Arc::new(IoUringInstance::new(sq_entries, cq_entries, owner));

    let process = crate::task::scheduler::get_current_process().ok_or(-14)?;
    let mut inner = process.inner.lock();
    let fd = inner.fd_table.iter().position(|s| s.is_none()).unwrap_or(inner.fd_table.len());
    if fd >= inner.fd_table.len() {
        inner.fd_table.resize_with(fd + 1, || None);
    }
    inner.fd_table[fd] = Some(crate::vfs::FileDescriptor {
        inode: crate::vfs::InodeId(0),
        backend: Arc::new(crate::fs::tmpfs::TmpfsBackend::new()),
        offset: core::sync::atomic::AtomicU64::new(0),
        flags: crate::vfs::OpenFlags::RDWR,
        kind: crate::vfs::FdKind::IoUring(instance.clone()),
        name: alloc::string::String::from("io_uring"),
    });
    drop(inner);

    IO_URING_INSTANCES.lock().insert(fd, instance);
    Ok(fd)
}

pub fn uring_enter(fd: usize, to_submit: u32, min_complete: u32, flags: u32) -> Result<u64, i32> {
    let instance = {
        let table = IO_URING_INSTANCES.lock();
        table.get(&fd).cloned().ok_or(-9)?
    };

    let _ = to_submit;

    instance.process_completions();

    if min_complete > 0 {
        let completed_count = instance.completed.lock().len() as u32;
        if completed_count >= min_complete {
            return Ok(completed_count as u64);
        }

        if flags & 1 != 0 {
            let task_id = crate::task::scheduler::get_current_task_id().ok_or(-14)?;
            instance.add_waiter(task_id);
            crate::task::scheduler::block_current();
            crate::task::scheduler::yield_task();
            instance.remove_waiter(task_id);

            instance.process_completions();
        }
    }

    let completed = instance.completed.lock().len() as u64;
    Ok(completed)
}

pub fn uring_register(fd: usize, opcode: u32, arg: u64) -> Result<u64, i32> {
    let instance = {
        let table = IO_URING_INSTANCES.lock();
        table.get(&fd).cloned().ok_or(-9)?
    };

    match opcode {
        0 => {
            *instance.notify_fd.lock() = Some(arg as usize);
            Ok(0)
        }
        1 => {
            *instance.notify_fd.lock() = None;
            Ok(0)
        }
        _ => Err(-22),
    }
}

pub fn get_instance(fd: usize) -> Option<Arc<IoUringInstance>> {
    IO_URING_INSTANCES.lock().get(&fd).cloned()
}

pub fn remove_instance(fd: usize) {
    IO_URING_INSTANCES.lock().remove(&fd);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_state_new() {
        let ring = RingState::new(8);
        assert_eq!(ring.entries, 8);
        assert_eq!(ring.mask, 7);
        assert_eq!(ring.available(), 0);
        assert_eq!(ring.space(), 8);
    }

    #[test]
    fn test_ring_state_submit_consume() {
        let ring = RingState::new(4);
        ring.tail.fetch_add(2, Ordering::Release);
        assert_eq!(ring.available(), 2);
        ring.head.fetch_add(1, Ordering::Release);
        assert_eq!(ring.available(), 1);
        assert_eq!(ring.space(), 3);
    }

    #[test]
    fn test_sqe_default() {
        let sqe = Sqe::default();
        assert_eq!(sqe.op, 0);
        assert_eq!(sqe.fd, -1);
        assert_eq!(sqe.addr, 0);
        assert_eq!(sqe.user_data, 0);
    }

    #[test]
    fn test_cqe_layout() {
        assert!(core::mem::size_of::<Cqe>() <= 16);
    }

    #[test]
    fn test_uring_op_conversion() {
        assert_eq!(IoUringOp::from(0), IoUringOp::Nop);
        assert_eq!(IoUringOp::from(1), IoUringOp::Read);
        assert_eq!(IoUringOp::from(2), IoUringOp::Write);
        assert_eq!(IoUringOp::from(11), IoUringOp::Timeout);
        assert_eq!(IoUringOp::from(99), IoUringOp::Nop);
    }

    #[test]
    fn test_instance_new() {
        let pid = ProcessId(42);
        let inst = IoUringInstance::new(16, 32, pid);
        assert_eq!(inst.sq_entries, 16);
        assert_eq!(inst.cq_entries, 32);
        assert_eq!(inst.owner, pid);
        assert!(inst.pending.lock().is_empty());
        assert!(inst.completed.lock().is_empty());
    }

    #[test]
    fn test_submit_sqes() {
        let inst = IoUringInstance::new(8, 16, ProcessId(1));
        let sqe = Sqe {
            op: 0,
            user_data: 42,
            ..Default::default()
        };
        let count = inst.submit_sqes(&[sqe]);
        assert_eq!(count, 1);
        assert_eq!(inst.pending.lock().len(), 1);
    }

    #[test]
    fn test_process_nop() {
        let inst = IoUringInstance::new(8, 16, ProcessId(1));
        let sqe = Sqe {
            op: 0,
            user_data: 99,
            ..Default::default()
        };
        inst.submit_sqes(&[sqe]);
        let processed = inst.process_completions();
        assert_eq!(processed, 1);
        let completed = inst.completed.lock();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].user_data, 99);
        assert_eq!(completed[0].res, 0);
    }

    #[test]
    fn test_invalid_op_rejected() {
        let inst = IoUringInstance::new(8, 16, ProcessId(1));
        let sqe = Sqe {
            op: 255,
            ..Default::default()
        };
        let count = inst.submit_sqes(&[sqe]);
        assert_eq!(count, 0);
    }
}
