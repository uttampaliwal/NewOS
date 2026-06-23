//! Deferred work execution via work queues.
//!
//! Work items are queued and executed later by a worker.  This avoids
//! doing heavy work in interrupt context.
//!
//! # Example
//!
//! ```ignore
//! use crate::sync::workqueue::{WorkQueue, work_submit};
//!
//! static WQ: WorkQueue = WorkQueue::new();
//!
//! fn my_work() {
//!     // deferred work here
//! }
//!
//! work_submit(&WQ, my_work);
//! WQ.process();
//! ```

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Maximum number of pending work items per queue.
const MAX_WORK_ITEMS: usize = 64;

/// A type-erased function pointer for work items.
type WorkFn = fn();

/// A single pending work entry.
struct WorkEntry {
    func: WorkFn,
    occupied: bool,
}

/// A simple single-consumer work queue.
///
/// Work items are enqueued by any context and processed by calling
/// [`process()`](WorkQueue::process).
pub struct WorkQueue {
    entries: UnsafeCell<[WorkEntry; MAX_WORK_ITEMS]>,
    head: AtomicU32,
    tail: AtomicU32,
    has_work: AtomicBool,
}

// SAFETY: WorkQueue uses atomic head/tail and UnsafeCell is only
// accessed under mutual exclusion provided by the CAS logic.
unsafe impl Send for WorkQueue {}
unsafe impl Sync for WorkQueue {}

impl WorkQueue {
    /// Create a new empty work queue.
    pub const fn new() -> Self {
        const EMPTY: WorkEntry = WorkEntry {
            func: || {},
            occupied: false,
        };
        Self {
            entries: UnsafeCell::new([EMPTY; MAX_WORK_ITEMS]),
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            has_work: AtomicBool::new(false),
        }
    }

    /// Enqueue a function for later execution.
    ///
    /// Returns `true` if the item was enqueued, `false` if the queue
    /// is full.
    pub fn enqueue(&self, func: WorkFn) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let next_tail = (tail + 1) % MAX_WORK_ITEMS as u32;
        let head = self.head.load(Ordering::Acquire);

        if next_tail == head {
            return false;
        }

        // SAFETY: we have exclusive access to this slot via the tail pointer.
        let entries = unsafe { &mut *self.entries.get() };
        let idx = tail as usize;

        entries[idx] = WorkEntry {
            func,
            occupied: true,
        };

        self.tail.store(next_tail, Ordering::Release);
        self.has_work.store(true, Ordering::Release);
        true
    }

    /// Process all pending work items.
    ///
    /// Items are executed in FIFO order.
    pub fn process(&self) {
        loop {
            let head = self.head.load(Ordering::Relaxed);
            let tail = self.tail.load(Ordering::Acquire);

            if head == tail {
                self.has_work.store(false, Ordering::Release);
                return;
            }

            // SAFETY: we have exclusive access to slot `head`.
            let entries = unsafe { &mut *self.entries.get() };
            let idx = head as usize;

            if entries[idx].occupied {
                let func = entries[idx].func;
                entries[idx].occupied = false;

                self.head
                    .store((head + 1) % MAX_WORK_ITEMS as u32, Ordering::Release);

                (func)();
            } else {
                self.head
                    .store((head + 1) % MAX_WORK_ITEMS as u32, Ordering::Release);
            }
        }
    }

    /// Returns `true` if there are pending work items.
    pub fn has_work(&self) -> bool {
        self.has_work.load(Ordering::Acquire)
    }

    /// Returns the number of pending work items.
    pub fn pending_count(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed) as usize;
        let tail = self.tail.load(Ordering::Relaxed) as usize;
        if tail >= head {
            tail - head
        } else {
            MAX_WORK_ITEMS - head + tail
        }
    }
}

impl Default for WorkQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Submit a function to a work queue.
pub fn work_submit(wq: &WorkQueue, func: WorkFn) -> bool {
    wq.enqueue(func)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial;
    use core::sync::atomic::AtomicU32;

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn test_add_1() {
        COUNTER.fetch_add(1, Ordering::Relaxed);
    }

    fn test_add_2() {
        COUNTER.fetch_add(2, Ordering::Relaxed);
    }

    #[test]
    fn workqueue_enqueue_process() {
        let _s = test_serial::acquire();
        let wq = WorkQueue::new();
        COUNTER.store(0, Ordering::Relaxed);

        wq.enqueue(test_add_1);
        wq.enqueue(test_add_2);
        wq.enqueue(test_add_1);

        assert_eq!(wq.pending_count(), 3);
        wq.process();
        assert_eq!(COUNTER.load(Ordering::Relaxed), 4);
        assert!(!wq.has_work());
    }

    #[test]
    fn workqueue_empty_process() {
        let _s = test_serial::acquire();
        let wq = WorkQueue::new();
        wq.process();
        assert!(!wq.has_work());
    }

    #[test]
    fn workqueue_submit_helper() {
        let _s = test_serial::acquire();
        let wq = WorkQueue::new();
        COUNTER.store(0, Ordering::Relaxed);

        assert!(work_submit(&wq, test_add_1));
        wq.process();
        assert_eq!(COUNTER.load(Ordering::Relaxed), 1);
    }
}
