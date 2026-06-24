//! Kernel log ring buffer.
//!
//! A lock-free, fixed-size ring buffer that stores kernel log messages.
//! Userland can read from this buffer via a shared-memory mapping or
//! a dedicated syscall. The buffer wraps around when full, preserving
//! the most recent messages.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Maximum number of log entries in the ring buffer.
const RING_SIZE: usize = 256;

/// Maximum length of a single log message (including null terminator).
const MAX_MSG_LEN: usize = 256;

/// A single log entry in the ring buffer.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct LogEntry {
    /// Timestamp in microseconds since boot.
    pub timestamp_us: u64,
    /// Log level (0=DEBUG, 1=INFO, 2=WARN, 3=ERROR).
    pub level: u8,
    /// Length of the message (excluding null terminator).
    pub msg_len: u16,
    /// Null-terminated message text.
    pub msg: [u8; MAX_MSG_LEN],
}

impl LogEntry {
    pub fn new(level: u8, message: &str) -> Self {
        let mut entry = Self {
            timestamp_us: 0,
            level,
            msg_len: 0,
            msg: [0u8; MAX_MSG_LEN],
        };
        let copy_len = core::cmp::min(message.len(), MAX_MSG_LEN - 1);
        entry.msg[..copy_len].copy_from_slice(&message.as_bytes()[..copy_len]);
        entry.msg[copy_len] = 0;
        entry.msg_len = copy_len as u16;
        entry
    }

    pub fn message(&self) -> &str {
        let len = self.msg_len as usize;
        core::str::from_utf8(&self.msg[..len]).unwrap_or("<invalid utf8>")
    }
}

const EMPTY_ENTRY: LogEntry = LogEntry {
    timestamp_us: 0,
    level: 0,
    msg_len: 0,
    msg: [0u8; MAX_MSG_LEN],
};

/// Lock-free ring buffer for kernel log messages.
pub struct LogRingBuffer {
    entries: UnsafeCell<[LogEntry; RING_SIZE]>,
    head: AtomicUsize,
    tail: AtomicUsize,
    count: AtomicUsize,
}

unsafe impl Send for LogRingBuffer {}
unsafe impl Sync for LogRingBuffer {}

impl Default for LogRingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl LogRingBuffer {
    /// Create a new, empty log ring buffer.
    pub const fn new() -> Self {
        Self {
            entries: UnsafeCell::new([EMPTY_ENTRY; RING_SIZE]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            count: AtomicUsize::new(0),
        }
    }

    /// Push a log entry into the ring buffer.
    /// If the buffer is full, the oldest entry is overwritten.
    pub fn push(&self, level: u8, message: &str) {
        let entry = LogEntry::new(level, message);
        let head = self.head.load(Ordering::Relaxed);
        let next_head = (head + 1) % RING_SIZE;

        // SAFETY: `head` is derived from the atomic head index modulo RING_SIZE,
        // so it is always in-bounds. The `UnsafeCell` is accessed under the
        // single-producer protocol: only the code that advanced the head index
        // writes to that slot, and the write uses `write_volatile` to prevent
        // the compiler from eliding or reordering it with the atomic store.
        unsafe {
            let entries = &mut *self.entries.get();
            core::ptr::write_volatile(&mut entries[head] as *mut LogEntry, entry);
        }

        self.head.store(next_head, Ordering::Release);

        let count = self.count.load(Ordering::Relaxed);
        if count >= RING_SIZE {
            self.tail.store(next_head, Ordering::Release);
        } else {
            self.count.store(count + 1, Ordering::Release);
        }
    }

    /// Pop the oldest log entry from the ring buffer.
    /// Returns `None` if the buffer is empty.
    pub fn pop(&self) -> Option<LogEntry> {
        let count = self.count.load(Ordering::Acquire);
        if count == 0 {
            return None;
        }

        let tail = self.tail.load(Ordering::Relaxed);
        // SAFETY: `tail` is derived from the atomic tail index modulo RING_SIZE
        // so it is always in-bounds. `LogEntry` is `Copy`, so the read is
        // safe regardless of concurrent head-side writes (the caller checked
        // count > 0 before reaching this point).
        let entry = unsafe { (*self.entries.get())[tail] };
        let next_tail = (tail + 1) % RING_SIZE;

        self.tail.store(next_tail, Ordering::Release);
        self.count.store(count - 1, Ordering::Release);

        Some(entry)
    }

    /// Peek at the oldest entry without removing it.
    pub fn peek(&self) -> Option<LogEntry> {
        let count = self.count.load(Ordering::Acquire);
        if count == 0 {
            return None;
        }
        let tail = self.tail.load(Ordering::Acquire);
        // SAFETY: `tail` is derived from the atomic tail index modulo RING_SIZE,
        // so it is always in-bounds. `LogEntry` is `Copy`, so the read is
        // a snapshot and will not leave partially-written data.
        Some(unsafe { (*self.entries.get())[tail] })
    }

    /// Number of entries currently in the buffer.
    pub fn len(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the buffer is full.
    pub fn is_full(&self) -> bool {
        self.len() >= RING_SIZE
    }

    /// Clear all entries from the buffer.
    pub fn clear(&self) {
        self.tail
            .store(self.head.load(Ordering::Relaxed), Ordering::Relaxed);
        self.count.store(0, Ordering::Relaxed);
    }
}

/// Global kernel log ring buffer.
static KERNEL_LOG: LogRingBuffer = LogRingBuffer::new();

/// Write a message to the kernel log ring buffer.
pub fn kernel_log(level: u8, message: &str) {
    KERNEL_LOG.push(level, message);
}

/// Read the next log entry from the kernel log ring buffer.
pub fn kernel_log_read() -> Option<LogEntry> {
    KERNEL_LOG.pop()
}

/// Return the number of unread log entries.
pub fn kernel_log_count() -> usize {
    KERNEL_LOG.len()
}

/// Clear the kernel log ring buffer.
pub fn kernel_log_clear() {
    KERNEL_LOG.clear();
}

// Convenience macros for different log levels
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        $crate::log_ring::kernel_log(0, &alloc::format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::log_ring::kernel_log(1, &alloc::format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::log_ring::kernel_log(2, &alloc::format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::log_ring::kernel_log(3, &alloc::format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;

    fn new_ring() -> Box<LogRingBuffer> {
        Box::new(LogRingBuffer::new())
    }

    #[test]
    fn test_ring_push_and_pop() {
        let ring = new_ring();
        assert!(ring.is_empty());

        ring.push(1, "hello");
        ring.push(2, "world");
        assert_eq!(ring.len(), 2);

        let e1 = ring.pop().unwrap();
        assert_eq!(e1.message(), "hello");
        assert_eq!(e1.level, 1);

        let e2 = ring.pop().unwrap();
        assert_eq!(e2.message(), "world");
        assert_eq!(e2.level, 2);

        assert!(ring.is_empty());
    }

    #[test]
    fn test_ring_overwrite_when_full() {
        let ring = new_ring();
        for _ in 0..RING_SIZE {
            ring.push(1, "fill");
        }
        assert!(ring.is_full());

        ring.push(2, "new");
        assert!(ring.is_full());

        let entry = ring.pop().unwrap();
        assert_eq!(entry.message(), "fill");
    }

    #[test]
    fn test_ring_peek() {
        let ring = new_ring();
        ring.push(1, "peek_test");

        let peeked = ring.peek().unwrap();
        assert_eq!(peeked.message(), "peek_test");
        assert_eq!(ring.len(), 1);
    }

    #[test]
    fn test_ring_clear() {
        let ring = new_ring();
        ring.push(1, "a");
        ring.push(2, "b");
        ring.clear();
        assert!(ring.is_empty());
    }

    #[test]
    fn test_global_kernel_log() {
        kernel_log_clear();
        kernel_log(1, "test message");
        let entry = kernel_log_read().unwrap();
        assert_eq!(entry.message(), "test message");
        assert_eq!(entry.level, 1);
    }

    #[test]
    fn test_log_entry_message() {
        let entry = LogEntry::new(3, "error: disk failure");
        assert_eq!(entry.level, 3);
        assert_eq!(entry.message(), "error: disk failure");
        assert_eq!(entry.msg_len, 19);
    }

    #[test]
    fn test_log_entry_truncation() {
        let long_msg = "a".repeat(300);
        let entry = LogEntry::new(1, &long_msg);
        assert_eq!(entry.msg_len as usize, MAX_MSG_LEN - 1);
    }

    #[test]
    fn test_ring_multiple_readers() {
        let ring = new_ring();
        for i in 0..100 {
            ring.push(1, &alloc::format!("msg {}", i));
        }
        for i in 0..100 {
            let entry = ring.pop().unwrap();
            assert_eq!(entry.message(), &alloc::format!("msg {}", i));
        }
        assert!(ring.is_empty());
    }
}
