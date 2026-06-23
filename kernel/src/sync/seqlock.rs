//! Sequence lock (seqlock) — optimistic reader / exclusive writer lock.
//!
//! Writers never block readers; instead readers retry if a writer was active
//! during their read.  This is ideal for read-heavy workloads where the
//! protected data is small and reads are short.
//!
//! # Example
//!
//! ```ignore
//! let lock = SeqLock::new(42u64);
//!
//! // Read side — optimistic, no atomics on the fast path.
//! let value = lock.read();
//!
//! // Write side — exclusive.
//! let mut guard = lock.write();
//! *guard = 100;
//! ```

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

/// A sequence lock protecting a value of type `T`.
///
/// The lock maintains a monotonically increasing sequence counter.
/// Writers increment it before and after mutation (odd = write in progress,
/// even = stable).  Readers check the counter before and after reading;
/// if it changed or is odd, they retry.
pub struct SeqLock<T> {
    seq: AtomicU32,
    data: UnsafeCell<T>,
}

// SAFETY: SeqLock provides mutual exclusion for writers and safe
// optimistic reads for readers through the sequence counter protocol.
unsafe impl<T: Send> Send for SeqLock<T> {}
unsafe impl<T: Send> Sync for SeqLock<T> {}

impl<T> SeqLock<T> {
    /// Create a new seqlock wrapping the given initial value.
    pub const fn new(val: T) -> Self {
        Self {
            seq: AtomicU32::new(0),
            data: UnsafeCell::new(val),
        }
    }

    /// Acquire a write guard.  The sequence counter is incremented to an
    /// odd value while the guard is held.
    pub fn write(&self) -> SeqWriteGuard<'_, T> {
        let old = self.seq.fetch_add(1, Ordering::Acquire);
        // Wait for any in-progress writer to finish (shouldn't happen
        // under single-writer semantics, but defensive).
        if old & 1 != 0 {
            while self.seq.load(Ordering::Acquire) & 1 != 0 {
                core::hint::spin_loop();
            }
        }
        SeqWriteGuard { lock: self }
    }

    /// Perform an optimistic read.  Retries if a write was in progress.
    pub fn read(&self) -> SeqReadGuard<'_, T> {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 != 0 {
                core::hint::spin_loop();
                continue;
            }
            return SeqReadGuard {
                lock: self,
                seq: s1,
            };
        }
    }

    /// Directly read the inner value, retrying on write contention.
    pub fn read_value(&self) -> T
    where
        T: Copy,
    {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 != 0 {
                core::hint::spin_loop();
                continue;
            }
            // SAFETY: s1 is even, so no writer is active.
            let val = unsafe { core::ptr::read(self.data.get()) };
            let s2 = self.seq.load(Ordering::Acquire);
            if s1 == s2 {
                return val;
            }
            core::hint::spin_loop();
        }
    }
}

/// Write guard — holds exclusive access and keeps the sequence odd.
pub struct SeqWriteGuard<'a, T> {
    lock: &'a SeqLock<T>,
}

impl<T> core::ops::Deref for SeqWriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: we hold the write lock, so exclusive access is guaranteed.
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> core::ops::DerefMut for SeqWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: we hold the write lock, so exclusive access is guaranteed.
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for SeqWriteGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.seq.fetch_add(1, Ordering::Release);
    }
}

/// Read guard — validates that no write occurred during the read.
pub struct SeqReadGuard<'a, T> {
    lock: &'a SeqLock<T>,
    seq: u32,
}

impl<T: Copy> SeqReadGuard<'_, T> {
    /// Read the protected value.  Panics (in debug) if a writer raced.
    pub fn get(&self) -> T {
        // SAFETY: we checked seq was even before constructing this guard.
        let val = unsafe { core::ptr::read(self.lock.data.get()) };
        let s2 = self.lock.seq.load(Ordering::Acquire);
        debug_assert_eq!(
            self.seq, s2,
            "seqlock: writer raced with reader during read"
        );
        val
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial;

    #[test]
    fn seqlock_write_read_roundtrip() {
        let _s = test_serial::acquire();
        let lock = SeqLock::new(0u32);

        {
            let mut w = lock.write();
            *w = 42;
        }

        let val = lock.read_value();
        assert_eq!(val, 42);
    }

    #[test]
    fn seqlock_read_guard_validates() {
        let _s = test_serial::acquire();
        let lock = SeqLock::new(100u64);

        let r = lock.read();
        assert_eq!(r.get(), 100u64);
    }

    #[test]
    fn seqlock_multiple_writes() {
        let _s = test_serial::acquire();
        let lock = SeqLock::new(0u32);

        for i in 0..100 {
            let mut w = lock.write();
            *w = i;
        }

        assert_eq!(lock.read_value(), 99);
    }
}
