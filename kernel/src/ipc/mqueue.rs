use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

pub const MQ_MAX_MSG: usize = 10;
pub const MQ_MSG_SIZE: usize = 256;

struct MqInner {
    buffer: Vec<(Vec<u8>, u32)>,
    max_msgs: usize,
    max_msg_size: usize,
    blocked_senders: Vec<crate::task::TaskId>,
    blocked_receivers: Vec<crate::task::TaskId>,
}

pub struct MessageQueue {
    inner: Mutex<MqInner>,
}

impl MessageQueue {
    pub fn new(_name: &str, max_msgs: usize, max_msg_size: usize) -> Self {
        MessageQueue {
            inner: Mutex::new(MqInner {
                buffer: Vec::new(),
                max_msgs,
                max_msg_size,
                blocked_senders: Vec::new(),
                blocked_receivers: Vec::new(),
            }),
        }
    }

    #[allow(clippy::never_loop)]
    pub fn send(&self, msg: &[u8], prio: u32) -> Result<(), i32> {
        if msg.len() > self.inner.lock().max_msg_size {
            return Err(92); // EMSGSIZE
        }
        loop {
            let mut inner = self.inner.lock();
            if inner.buffer.len() < inner.max_msgs {
                inner.buffer.push((msg.to_vec(), prio));
                let wakers = core::mem::take(&mut inner.blocked_receivers);
                drop(inner);
                for tid in wakers {
                    crate::task::scheduler::wake_task_by_id(tid);
                }
                crate::ipc::epoll::notify_all_epoll_waiters();
                return Ok(());
            }
            #[cfg(test)]
            {
                return Err(11); // EAGAIN
            }
            #[cfg(not(test))]
            {
                if let Some(tid) = crate::task::scheduler::get_current_task_id() {
                    inner.blocked_senders.push(tid);
                }
                drop(inner);
                crate::task::scheduler::block_current();
                crate::task::scheduler::yield_task();
            }
        }
    }

    #[allow(clippy::never_loop)]
    pub fn receive(&self, buf: &mut [u8]) -> Result<(usize, u32), i32> {
        loop {
            let mut inner = self.inner.lock();
            if let Some(idx) = inner
                .buffer
                .iter()
                .enumerate()
                .min_by_key(|(_, (_, p))| *p)
                .map(|(i, _)| i)
            {
                let (msg, prio) = inner.buffer.remove(idx);
                let n = core::cmp::min(msg.len(), buf.len());
                buf[..n].copy_from_slice(&msg[..n]);
                let wakers = core::mem::take(&mut inner.blocked_senders);
                drop(inner);
                for tid in wakers {
                    crate::task::scheduler::wake_task_by_id(tid);
                }
                return Ok((n, prio));
            }
            #[cfg(test)]
            {
                return Err(11); // EAGAIN
            }
            #[cfg(not(test))]
            {
                if let Some(tid) = crate::task::scheduler::get_current_task_id() {
                    inner.blocked_receivers.push(tid);
                }
                drop(inner);
                crate::task::scheduler::block_current();
                crate::task::scheduler::yield_task();
            }
        }
    }

    pub fn try_send(&self, msg: &[u8], prio: u32) -> Result<(), i32> {
        let mut inner = self.inner.lock();
        if msg.len() > inner.max_msg_size {
            return Err(92); // EMSGSIZE
        }
        if inner.buffer.len() >= inner.max_msgs {
            return Err(11); // EAGAIN
        }
        inner.buffer.push((msg.to_vec(), prio));
        let wakers = core::mem::take(&mut inner.blocked_receivers);
        drop(inner);
        for tid in wakers {
            crate::task::scheduler::wake_task_by_id(tid);
        }
        crate::ipc::epoll::notify_all_epoll_waiters();
        Ok(())
    }

    pub fn try_receive(&self, buf: &mut [u8]) -> Result<(usize, u32), i32> {
        let mut inner = self.inner.lock();
        if let Some(idx) = inner
            .buffer
            .iter()
            .enumerate()
            .min_by_key(|(_, (_, p))| *p)
            .map(|(i, _)| i)
        {
            let (msg, prio) = inner.buffer.remove(idx);
            let n = core::cmp::min(msg.len(), buf.len());
            buf[..n].copy_from_slice(&msg[..n]);
            let wakers = core::mem::take(&mut inner.blocked_senders);
            drop(inner);
            for tid in wakers {
                crate::task::scheduler::wake_task_by_id(tid);
            }
            crate::ipc::epoll::notify_all_epoll_waiters();
            return Ok((n, prio));
        }
        Err(11) // EAGAIN
    }
}

static MQUEUES: Mutex<alloc::collections::BTreeMap<String, Arc<MessageQueue>>> =
    Mutex::new(alloc::collections::BTreeMap::new());

pub fn mq_open(name: &str, flags: i32, _mode: u32, max_msgs: usize, max_msg_size: usize) -> Result<Arc<MessageQueue>, i32> {
    let mut queues = MQUEUES.lock();
    if let Some(q) = queues.get(name) {
        let q = Arc::clone(q);
        drop(queues);
        return Ok(q);
    }
    if flags & 0x100 == 0 {
        // O_CREAT not set
        return Err(2); // ENOENT
    }
    let q = Arc::new(MessageQueue::new(name, max_msgs, max_msg_size));
    queues.insert(String::from(name), Arc::clone(&q));
    drop(queues);
    Ok(q)
}

pub fn mq_unlink(name: &str) -> Result<(), i32> {
    let mut queues = MQUEUES.lock();
    queues.remove(name).ok_or(2)?; // ENOENT
    Ok(())
}
