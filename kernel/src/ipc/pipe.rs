use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

pub const PIPE_BUF_SIZE: usize = 65536;

struct PipeInner {
    buffer: *mut u8,
    read_pos: usize,
    write_pos: usize,
    bytes_available: usize,
}

unsafe impl Send for PipeInner {}
unsafe impl Sync for PipeInner {}

impl PipeInner {
    fn new() -> Self {
        let buf = crate::memory::slab::slab_alloc(PIPE_BUF_SIZE)
            .expect("pipe: failed to allocate buffer from slab");
        // Safety: buf is a valid pointer returned from slab_alloc with at least PIPE_BUF_SIZE bytes.
        unsafe {
            core::ptr::write_bytes(buf, 0, PIPE_BUF_SIZE);
        }
        PipeInner {
            buffer: buf,
            read_pos: 0,
            write_pos: 0,
            bytes_available: 0,
        }
    }

    fn read(&mut self, buf: &mut [u8]) -> usize {
        let to_read = core::cmp::min(self.bytes_available, buf.len());
        for (i, byte) in buf.iter_mut().enumerate().take(to_read) {
            let idx = (self.read_pos + i) % PIPE_BUF_SIZE;
            // Safety: buffer is a valid slab-allocated pointer; idx is within 0..PIPE_BUF_SIZE.
            unsafe {
                *byte = self.buffer.add(idx).read_volatile();
            }
        }
        self.read_pos = (self.read_pos + to_read) % PIPE_BUF_SIZE;
        self.bytes_available -= to_read;
        to_read
    }

    fn write(&mut self, buf: &[u8]) -> usize {
        let space = PIPE_BUF_SIZE - self.bytes_available;
        let to_write = core::cmp::min(space, buf.len());
        for (i, byte) in buf.iter().enumerate().take(to_write) {
            let idx = (self.write_pos + i) % PIPE_BUF_SIZE;
            // Safety: buffer is a valid slab-allocated pointer; idx is within 0..PIPE_BUF_SIZE.
            unsafe {
                self.buffer.add(idx).write_volatile(*byte);
            }
        }
        self.write_pos = (self.write_pos + to_write) % PIPE_BUF_SIZE;
        self.bytes_available += to_write;
        to_write
    }

    fn bytes_available(&self) -> usize {
        self.bytes_available
    }

    fn space_available(&self) -> usize {
        PIPE_BUF_SIZE - self.bytes_available
    }
}

impl Drop for PipeInner {
    fn drop(&mut self) {
        crate::memory::slab::slab_dealloc(self.buffer, PIPE_BUF_SIZE);
    }
}

pub struct PipeBuffer {
    inner: Mutex<PipeInner>,
    pub(crate) write_end_open: AtomicBool,
    pub(crate) read_end_open: AtomicBool,
    blocked_writers: Mutex<alloc::vec::Vec<crate::task::TaskId>>,
}

impl Default for PipeBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl PipeBuffer {
    pub fn new() -> Self {
        PipeBuffer {
            inner: Mutex::new(PipeInner::new()),
            write_end_open: AtomicBool::new(true),
            read_end_open: AtomicBool::new(true),
            blocked_writers: Mutex::new(alloc::vec::Vec::new()),
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> usize {
        let mut inner = self.inner.lock();
        let n = inner.read(buf);
        // After consuming data, wake any blocked writers.
        if n > 0 {
            let wakers = core::mem::take(&mut *self.blocked_writers.lock());
            for tid in wakers {
                crate::task::scheduler::wake_task_by_id(tid);
            }
            crate::ipc::epoll::notify_all_epoll_waiters();
        }
        n
    }

    pub fn write(&self, buf: &[u8]) -> usize {
        if !self.is_read_end_open() {
            return 0;
        }
        let mut inner = self.inner.lock();
        let n = inner.write(buf);
        if n > 0 {
            crate::ipc::epoll::notify_all_epoll_waiters();
        }
        n
    }

    pub fn write_blocking(&self, buf: &[u8]) -> usize {
        #[allow(clippy::never_loop)]
        loop {
            // Try a non-blocking write first.
            let mut inner = self.inner.lock();
            let n = inner.write(buf);
            if n > 0 || buf.is_empty() {
                return n;
            }
            // Buffer is full — if read end is closed, deliver SIGPIPE.
            if !self.is_read_end_open() {
                if let Some(process) = crate::task::scheduler::get_current_process() {
                    let mut inner2 = process.inner.lock();
                    inner2.pending_signals.insert(13); // SIGPIPE = 13
                }
                return 0;
            }
            // In test mode the scheduler cannot unblock us, so return 0.
            #[cfg(test)]
            {
                let _ = inner;
                return 0;
            }
            #[cfg(not(test))]
            {
                drop(inner);
                // Register this task as a blocked writer.
                if let Some(tid) = crate::task::scheduler::get_current_task_id() {
                    self.blocked_writers.lock().push(tid);
                }
                // Block until woken by a reader.
                crate::task::scheduler::block_current();
                crate::task::scheduler::yield_task();
                // When woken, loop back and retry.
            }
        }
    }

    pub fn bytes_available(&self) -> usize {
        let inner = self.inner.lock();
        inner.bytes_available()
    }

    pub fn space_available(&self) -> usize {
        let inner = self.inner.lock();
        inner.space_available()
    }

    pub fn is_write_end_open(&self) -> bool {
        self.write_end_open.load(Ordering::Acquire)
    }

    pub fn is_read_end_open(&self) -> bool {
        self.read_end_open.load(Ordering::Acquire)
    }

    pub fn close_read_end(&self) {
        self.read_end_open.store(false, Ordering::Release);
        // Wake any blocked writers so they can detect the closed read end.
        let wakers = core::mem::take(&mut *self.blocked_writers.lock());
        for tid in wakers {
            crate::task::scheduler::wake_task_by_id(tid);
        }
    }

    pub fn close_write_end(&self) {
        self.write_end_open.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ------------------------------------------------------------------
    // Property 16 — Pipe Data Integrity
    // ------------------------------------------------------------------
    //
    // For any arbitrary byte sequence:
    //   1. Writing the sequence to a pipe succeeds.
    //   2. Reading from the pipe returns the same bytes in the same order.
    //
    // Validates Requirements 18.1.

    proptest! {
        #[test]
        fn pipe_data_integrity(data in proptest::collection::vec(0u8..=255, 0..PIPE_BUF_SIZE)) {
            let pipe = PipeBuffer::new();
            let original_len = data.len();

            // Write all data into the pipe.
            let written = pipe.write(&data);
            prop_assert_eq!(written, original_len, "write must accept all bytes");

            // Read back everything.
            let mut out = alloc::vec![0u8; original_len];
            let read = pipe.read(&mut out);
            prop_assert_eq!(read, original_len, "read must return all bytes");

            // Verify byte-for-byte equality.
            prop_assert_eq!(&out[..read], &data[..]);
        }
    }

    // ------------------------------------------------------------------
    // Property 17 — Pipe Blocking on Full Buffer
    // ------------------------------------------------------------------
    //
    // When the pipe buffer is full:
    //   1. A write returns 0 (no space available).
    //   2. After a reader consumes at least one byte, a write succeeds.
    //
    // Validates Requirements 18.3.

    proptest! {
        #[test]
        fn pipe_blocking_on_full_buffer(tail_len in 1..=4096usize) {
            let pipe = PipeBuffer::new();

            // Fill the pipe to capacity.
            let fill_data = alloc::vec![0xABu8; PIPE_BUF_SIZE];
            let written = pipe.write(&fill_data);
            prop_assert_eq!(written, PIPE_BUF_SIZE, "must fill pipe completely");

            // Verify no space remains.
            prop_assert_eq!(pipe.space_available(), 0, "pipe must be full");

            // Attempt to write more data — must return 0 (no space).
            let extra = alloc::vec![0xCDu8; tail_len];
            let extra_written = pipe.write(&extra);
            prop_assert_eq!(extra_written, 0, "write to full pipe must return 0");

            // Read one byte to make space.
            let mut single = [0u8; 1];
            let n = pipe.read(&mut single);
            prop_assert_eq!(n, 1, "must read exactly 1 byte");
            prop_assert_eq!(single[0], 0xAB, "byte must match what was written");

            // Now a write should succeed (at least 1 byte of space).
            let recovered = pipe.write(&extra);
            prop_assert!(recovered > 0, "write must succeed after drain");
            prop_assert_eq!(recovered, 1,
                "must write exactly 1 byte (only 1 was freed)");

            // Read back everything and verify content.
            let mut out = alloc::vec![0u8; PIPE_BUF_SIZE - 1 + recovered];
            let total_read = pipe.read(&mut out);
            prop_assert_eq!(total_read, out.len(), "must drain all data");

            // First (PIPE_BUF_SIZE - 1) bytes should still be 0xAB.
            for &byte in out[..PIPE_BUF_SIZE - 1].iter() {
                prop_assert_eq!(byte, 0xAB, "original data must be preserved");
            }
            // Last `recovered` bytes should be 0xCD.
            for &byte in out[PIPE_BUF_SIZE - 1..].iter() {
                prop_assert_eq!(byte, 0xCD, "new data must be preserved");
            }
        }
    }

    #[test]
    fn pipe_empty_read_returns_zero() {
        let pipe = PipeBuffer::new();
        let mut buf = [0u8; 16];
        let n = pipe.read(&mut buf);
        assert_eq!(n, 0, "read from empty pipe must return 0");
    }

    #[test]
    fn pipe_write_read_roundtrip() {
        let pipe = PipeBuffer::new();
        let input = b"hello pipe";
        let written = pipe.write(input);
        assert_eq!(written, input.len());

        let mut output = [0u8; 32];
        let read = pipe.read(&mut output);
        assert_eq!(read, input.len());
        assert_eq!(&output[..read], input);
    }

    #[test]
    fn pipe_wraparound() {
        let pipe = PipeBuffer::new();
        // Write enough to force at least one wrap of the ring buffer.
        let mut buf = alloc::vec![0u8; PIPE_BUF_SIZE];
        // Fill with a pattern.
        for (i, b) in buf.iter_mut().enumerate() {
            *b = (i & 0xFF) as u8;
        }
        let written = pipe.write(&buf);
        assert_eq!(written, PIPE_BUF_SIZE);

        // Read half.
        let mut first_half = alloc::vec![0u8; PIPE_BUF_SIZE / 2];
        let n = pipe.read(&mut first_half);
        assert_eq!(n, PIPE_BUF_SIZE / 2);
        for (i, b) in first_half.iter().enumerate().take(n) {
            assert_eq!(*b, (i & 0xFF) as u8, "mismatch at offset {}", i);
        }

        // Write more data to force wrap-around position to change.
        for (i, b) in buf.iter_mut().enumerate().take(PIPE_BUF_SIZE / 2) {
            *b = ((i + 0x80) & 0xFF) as u8;
        }
        let written2 = pipe.write(&buf[..PIPE_BUF_SIZE / 2]);
        assert_eq!(written2, PIPE_BUF_SIZE / 2);

        // Read everything remaining and verify order.
        let mut full = alloc::vec![0u8; PIPE_BUF_SIZE];
        let total = pipe.read(&mut full);
        assert_eq!(total, PIPE_BUF_SIZE);

        // Second half of original.
        for (i, b) in full.iter().enumerate().take(PIPE_BUF_SIZE / 2) {
            assert_eq!(
                *b,
                ((i + PIPE_BUF_SIZE / 2) & 0xFF) as u8,
                "original second half mismatch at {}",
                i
            );
        }
        // Then the new data.
        for i in 0..PIPE_BUF_SIZE / 2 {
            assert_eq!(
                full[PIPE_BUF_SIZE / 2 + i],
                ((i + 0x80) & 0xFF) as u8,
                "new data mismatch at {}",
                i
            );
        }
    }

    #[test]
    fn pipe_close_write_end_returns_eof_on_read() {
        let pipe = PipeBuffer::new();
        pipe.write(b"data");
        pipe.close_write_end();
        let mut buf = [0u8; 16];
        let n = pipe.read(&mut buf);
        assert_eq!(n, 4);
        assert_eq!(&buf[..4], b"data");
        // Second read returns 0 (EOF)
        let n2 = pipe.read(&mut buf);
        assert_eq!(n2, 0);
    }

    #[test]
    fn pipe_close_read_end_returns_zero_on_write() {
        let pipe = PipeBuffer::new();
        // Fill pipe first
        let fill = alloc::vec![0u8; PIPE_BUF_SIZE];
        pipe.write(&fill);
        pipe.close_read_end();
        let n = pipe.write(b"hello");
        assert_eq!(n, 0, "write after close_read_end should return 0");
    }

    #[test]
    fn pipe_close_read_end_without_data_returns_zero_on_write() {
        let pipe = PipeBuffer::new();
        pipe.close_read_end();
        let n = pipe.write(b"hello");
        assert_eq!(n, 0);
    }

    #[test]
    fn pipe_write_end_open_after_close() {
        let pipe = PipeBuffer::new();
        assert!(pipe.is_write_end_open());
        pipe.close_write_end();
        assert!(!pipe.is_write_end_open());
    }

    #[test]
    fn pipe_read_end_open_after_close() {
        let pipe = PipeBuffer::new();
        assert!(pipe.is_read_end_open());
        pipe.close_read_end();
        assert!(!pipe.is_read_end_open());
    }

    #[test]
    fn pipe_bytes_available_tracking() {
        let pipe = PipeBuffer::new();
        assert_eq!(pipe.bytes_available(), 0);
        pipe.write(b"abc");
        assert_eq!(pipe.bytes_available(), 3);
        let mut buf = [0u8; 2];
        pipe.read(&mut buf);
        assert_eq!(pipe.bytes_available(), 1);
        pipe.read(&mut buf);
        assert_eq!(pipe.bytes_available(), 0);
    }

    #[test]
    fn pipe_space_available_tracking() {
        let pipe = PipeBuffer::new();
        assert_eq!(pipe.space_available(), PIPE_BUF_SIZE);
        pipe.write(&alloc::vec![0u8; 100]);
        assert_eq!(pipe.space_available(), PIPE_BUF_SIZE - 100);
        let mut buf = [0u8; 50];
        pipe.read(&mut buf);
        assert_eq!(pipe.space_available(), PIPE_BUF_SIZE - 50);
    }
}
