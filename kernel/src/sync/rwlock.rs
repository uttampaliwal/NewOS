//! Read-write lock — multiple concurrent readers or one exclusive writer.
//!
//! Multiple readers can hold the lock simultaneously; a writer gets
//! exclusive access.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

/// A readers-writer lock.  Multiple readers can hold the lock
/// simultaneously; a writer gets exclusive access.
pub struct RwLock<T> {
    /// Bit 0: write lock held.  Bits 1..=31: number of readers.
    state: AtomicU32,
    data: UnsafeCell<T>,
}

// SAFETY: RwLock provides mutual exclusion for writers and shared
// access for readers through atomic state tracking.
unsafe impl<T: Send> Send for RwLock<T> {}
unsafe impl<T: Send + Sync> Sync for RwLock<T> {}

impl<T> RwLock<T> {
    /// Create a new read-write lock wrapping the given value.
    pub const fn new(val: T) -> Self {
        Self {
            state: AtomicU32::new(0),
            data: UnsafeCell::new(val),
        }
    }

    /// Acquire a read lock.  Blocks (spins) if a writer holds the lock.
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        loop {
            let state = self.state.load(Ordering::Acquire);
            if state & 1 == 0 {
                if self
                    .state
                    .compare_exchange_weak(state, state + 2, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    return RwLockReadGuard { lock: self };
                }
            } else {
                core::hint::spin_loop();
            }
        }
    }

    /// Acquire a write lock.  Blocks (spins) until all readers and
    /// any previous writer release.
    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        loop {
            if self
                .state
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Acquire)
                .is_ok()
            {
                return RwLockWriteGuard { lock: self };
            }
            core::hint::spin_loop();
        }
    }

    /// Try to acquire a read lock without blocking.
    pub fn try_read(&self) -> Option<RwLockReadGuard<'_, T>> {
        let state = self.state.load(Ordering::Acquire);
        if state & 1 == 0
            && self
                .state
                .compare_exchange(state, state + 2, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            return Some(RwLockReadGuard { lock: self });
        }
        None
    }

    /// Try to acquire a write lock without blocking.
    pub fn try_write(&self) -> Option<RwLockWriteGuard<'_, T>> {
        if self
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Acquire)
            .is_ok()
        {
            Some(RwLockWriteGuard { lock: self })
        } else {
            None
        }
    }
}

/// Read guard — holds a shared reference.
pub struct RwLockReadGuard<'a, T> {
    lock: &'a RwLock<T>,
}

impl<T> core::ops::Deref for RwLockReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: We hold a read lock (state >= 2, bit 0 clear), so no writer
        // is active. Shared immutable access is safe for multiple readers.
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.state.fetch_sub(2, Ordering::Release);
    }
}

/// Write guard — holds exclusive access.
pub struct RwLockWriteGuard<'a, T> {
    lock: &'a RwLock<T>,
}

impl<T> core::ops::Deref for RwLockWriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: we hold the write lock.
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> core::ops::DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: we hold the write lock.
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.state.store(0, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial;

    #[test]
    fn rwlock_write_read_roundtrip() {
        let _s = test_serial::acquire();
        let lock = RwLock::new(0u32);

        {
            let mut w = lock.write();
            *w = 42;
        }

        let r = lock.read();
        assert_eq!(*r, 42);
    }

    #[test]
    fn rwlock_try_read() {
        let _s = test_serial::acquire();
        let lock = RwLock::new(10u32);

        let r = lock.try_read().unwrap();
        assert_eq!(*r, 10);
    }

    #[test]
    fn rwlock_try_write() {
        let _s = test_serial::acquire();
        let lock = RwLock::new(10u32);

        let w = lock.try_write().unwrap();
        assert_eq!(*w, 10);
    }

    #[test]
    fn rwlock_try_write_blocked_by_reader() {
        let _s = test_serial::acquire();
        let lock = RwLock::new(10u32);

        let _r = lock.read();
        assert!(lock.try_write().is_none());
    }
}
