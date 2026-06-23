//! RCU (Read-Copy-Update) synchronization primitive.
//!
//! RCU allows multiple readers to access shared data concurrently without
//! locking, while writers create copies and atomically swap pointers.
//!
//! # Overview
//!
//! ```ignore
//! use crate::sync::rcu::RcuPtr;
//!
//! static ITEMS: RcuPtr<u32> = RcuPtr::new(&10);
//!
//! let val = ITEMS.read().get();
//! ITEMS.publish(&20);
//! ```
//!
//! # Safety Model
//!
//! - `rcu_read_lock()` / `rcu_read_unlock()` bracket the critical section.
//! - `synchronize_rcu()` blocks until all pre-existing readers complete.
//! - `call_rcu()` registers a callback to run after grace period.

use core::marker::PhantomData;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

/// Per-CPU (simplified for single-CPU) read nesting counter.
static RCU_READ_NESTING: AtomicU32 = AtomicU32::new(0);

/// Grace period counter.  Incremented by `synchronize_rcu()`.
static RCU_GP_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Maximum registered callbacks.
const MAX_RCU_CALLBACKS: usize = 32;

/// Wrapper to make raw pointers Send+Sync for use inside Mutex.
struct SyncPtr(*mut ());
// SAFETY: access to SyncPtr is protected by the CALLBACKS Mutex.
unsafe impl Send for SyncPtr {}
unsafe impl Sync for SyncPtr {}

/// A deferred RCU callback.
struct RcuCallback {
    func: fn(*mut ()),
    arg: SyncPtr,
    gp_seen: u32,
    occupied: bool,
}

static CALLBACKS: spin::Mutex<[RcuCallback; MAX_RCU_CALLBACKS]> = spin::Mutex::new({
    const EMPTY: RcuCallback = RcuCallback {
        func: |_| {},
        arg: SyncPtr(core::ptr::null_mut()),
        gp_seen: 0,
        occupied: false,
    };
    [EMPTY; MAX_RCU_CALLBACKS]
});

/// Enter an RCU read-side critical section.
pub fn rcu_read_lock() {
    RCU_READ_NESTING.fetch_add(1, Ordering::Relaxed);
}

/// Exit an RCU read-side critical section.
pub fn rcu_read_unlock() {
    let prev = RCU_READ_NESTING.fetch_sub(1, Ordering::Release);
    debug_assert!(prev > 0, "rcu_read_unlock without matching read_lock");
}

/// Block until all pre-existing RCU readers have completed.
pub fn synchronize_rcu() {
    while RCU_READ_NESTING.load(Ordering::Acquire) != 0 {
        core::hint::spin_loop();
    }
    RCU_GP_COUNTER.fetch_add(1, Ordering::Release);
    fire_callbacks();
}

/// Register a callback to be invoked after the next grace period.
pub fn call_rcu(func: fn(*mut ()), arg: *mut ()) {
    let gp = RCU_GP_COUNTER.load(Ordering::Acquire);
    let mut callbacks = CALLBACKS.lock();

    for entry in callbacks.iter_mut() {
        if !entry.occupied {
            entry.func = func;
            entry.arg = SyncPtr(arg);
            entry.gp_seen = gp;
            entry.occupied = true;
            return;
        }
    }
}

fn fire_callbacks() {
    let gp = RCU_GP_COUNTER.load(Ordering::Acquire);
    let mut callbacks = CALLBACKS.lock();

    for entry in callbacks.iter_mut() {
        if entry.occupied && gp > entry.gp_seen {
            (entry.func)(entry.arg.0);
            entry.occupied = false;
        }
    }
}

/// A wrapper for RCU-protected pointers.
///
/// `RcuPtr<T>` holds a pointer to `T` that can be read lock-free and
/// atomically replaced by writers.
pub struct RcuPtr<T> {
    ptr: AtomicPtr<T>,
}

impl<T> RcuPtr<T> {
    /// Create a new RCU pointer from a static reference.
    pub const fn new(val: &'static T) -> Self {
        Self {
            ptr: AtomicPtr::new(val as *const T as *mut T),
        }
    }

    /// Read the current value via a read guard.
    pub fn read(&self) -> RcuReadGuard<'_, T> {
        let p = self.ptr.load(Ordering::Acquire);
        RcuReadGuard {
            ptr: p,
            _marker: PhantomData,
        }
    }

    /// Publish a new value, replacing the old one.
    pub fn publish(&self, new: &'static T) {
        let old = self.ptr.swap(new as *const T as *mut T, Ordering::AcqRel);
        let _ = old;
    }
}

/// Read guard for RCU-protected data.
pub struct RcuReadGuard<'a, T> {
    ptr: *const T,
    _marker: PhantomData<&'a T>,
}

impl<T: Copy> RcuReadGuard<'_, T> {
    /// Read the protected value.
    pub fn get(&self) -> T {
        // SAFETY: the pointer was valid when loaded via Acquire.
        unsafe { core::ptr::read(self.ptr) }
    }
}

impl<T> RcuReadGuard<'_, T> {
    /// Access the inner pointer directly.
    pub fn as_ptr(&self) -> *const T {
        self.ptr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial;

    #[test]
    fn rcu_read_lock_unlock() {
        let _s = test_serial::acquire();
        rcu_read_lock();
        assert_eq!(RCU_READ_NESTING.load(Ordering::Relaxed), 1);
        rcu_read_unlock();
        assert_eq!(RCU_READ_NESTING.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn rcu_ptr_publish_read() {
        let _s = test_serial::acquire();
        static VAL_A: u32 = 10;
        static VAL_B: u32 = 20;
        let ptr = RcuPtr::new(&VAL_A);

        let r = ptr.read();
        assert_eq!(r.get(), 10);

        ptr.publish(&VAL_B);

        let r = ptr.read();
        assert_eq!(r.get(), 20);
    }

    #[test]
    fn rcu_synchronize_completes() {
        let _s = test_serial::acquire();
        let before = RCU_GP_COUNTER.load(Ordering::Relaxed);
        synchronize_rcu();
        let after = RCU_GP_COUNTER.load(Ordering::Relaxed);
        assert!(after > before);
    }

    static CALLBACK_FIRED: AtomicU32 = AtomicU32::new(0);

    fn test_callback(_arg: *mut ()) {
        CALLBACK_FIRED.fetch_add(1, Ordering::Relaxed);
    }

    #[test]
    fn rcu_call_rcu_fires_after_gp() {
        let _s = test_serial::acquire();
        CALLBACK_FIRED.store(0, Ordering::Relaxed);
        call_rcu(test_callback, core::ptr::null_mut());
        synchronize_rcu();
        assert_eq!(CALLBACK_FIRED.load(Ordering::Relaxed), 1);
    }
}
