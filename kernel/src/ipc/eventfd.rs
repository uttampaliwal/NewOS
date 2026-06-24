//! eventfd — kernel-to-userspace event notification.
//!
//! An eventfd is a lightweight file descriptor that supports
//! read/write of a 64-bit unsigned integer counter.  Multiple writers
//! can add to the counter; a reader atomically resets it to zero and
//! returns the old value.

use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::task::TaskId;
use crate::task::scheduler::{block_current, get_current_task_id, wake_task_by_id};

/// Maximum value for the eventfd counter (2^64 - 1).
const EFD_MAX_COUNTER: u64 = u64::MAX - 1;

/// An eventfd instance.
pub struct EventFd {
    /// The event counter.
    counter: AtomicU64,
    /// Tasks waiting for the counter to become non-zero.
    waiters: Mutex<VecDeque<TaskId>>,
}

impl EventFd {
    /// Create a new eventfd with the given initial value.
    pub fn new(initval: u64) -> Self {
        Self {
            counter: AtomicU64::new(initval),
            waiters: Mutex::new(VecDeque::new()),
        }
    }

    /// Write a 64-bit value to the counter (adds to it).
    ///
    /// If the counter would overflow, blocks until a read resets it.
    #[allow(clippy::result_unit_err)]
    pub fn write_value(&self, val: u64) -> Result<(), ()> {
        if val == u64::MAX {
            return Err(());
        }

        loop {
            let current = self.counter.load(Ordering::Acquire);
            let new = current.saturating_add(val);
            if new > EFD_MAX_COUNTER {
                if let Some(tid) = get_current_task_id() {
                    self.waiters.lock().push_back(tid);
                    block_current();
                }
                continue;
            }
            if self
                .counter
                .compare_exchange(current, new, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                self.notify_waiters();
                return Ok(());
            }
        }
    }

    /// Read the counter atomically.  Resets it to zero and returns the
    /// old value.
    #[allow(clippy::result_unit_err)]
    pub fn read_value(&self) -> Result<u64, ()> {
        loop {
            let current = self.counter.load(Ordering::Acquire);
            if current == 0 {
                if let Some(tid) = get_current_task_id() {
                    self.waiters.lock().push_back(tid);
                    block_current();
                }
                continue;
            }
            if self
                .counter
                .compare_exchange(current, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(current);
            }
        }
    }

    /// Return the current counter value without modifying it.
    pub fn peek(&self) -> u64 {
        self.counter.load(Ordering::Relaxed)
    }

    /// Poll for readability (counter > 0).
    pub fn poll(&self) -> u8 {
        if self.counter.load(Ordering::Acquire) > 0 {
            0x01 // EPOLLIN
        } else {
            0
        }
    }

    fn notify_waiters(&self) {
        let mut waiters = self.waiters.lock();
        while let Some(task_id) = waiters.pop_front() {
            wake_task_by_id(task_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial;

    #[test]
    fn eventfd_write_read() {
        let _s = test_serial::acquire();
        let efd = EventFd::new(0);
        efd.write_value(5).unwrap();
        efd.write_value(3).unwrap();
        assert_eq!(efd.peek(), 8);
        assert_eq!(efd.read_value().unwrap(), 8);
        assert_eq!(efd.peek(), 0);
    }

    #[test]
    fn eventfd_initial_value() {
        let _s = test_serial::acquire();
        let efd = EventFd::new(42);
        assert_eq!(efd.peek(), 42);
        assert_eq!(efd.read_value().unwrap(), 42);
    }

    #[test]
    fn eventfd_poll() {
        let _s = test_serial::acquire();
        let efd = EventFd::new(0);
        assert_eq!(efd.poll(), 0);
        efd.write_value(1).unwrap();
        assert_eq!(efd.poll(), 0x01);
    }
}
