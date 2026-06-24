//! Kernel Electric Fence (KFENCE) — low-overhead, sampling-based memory safety detector.
//!
//! Unlike KASAN which instruments every access, KFENCE randomly promotes a fraction
//! of kernel allocations to *fenced slots*. Each fenced slot has a fixed-size data
//! region surrounded by 16-byte redzones filled with [`KFENCE_REDZONE_BYTE`]. After
//! the object is freed the data region is overwritten with `0xBB` so that any
//! subsequent write can be caught by [`kfence_check_canaries`].
//!
//! # Sampling
//! A global [`AtomicU64`] counter is incremented on every call to [`kfence_alloc`].
//! When `counter % KFENCE_SAMPLE_INTERVAL == 0` the allocation is redirected to a
//! free KFENCE slot; all other calls return [`None`] immediately so the caller falls
//! back to its normal allocator.
//!
//! # Limitations
//! * Only allocations up to [`KFENCE_OBJECT_SIZE`] bytes are eligible.
//! * The pool has exactly [`KFENCE_POOL_SIZE`] slots; if all are in use the sampled
//!   allocation is silently skipped.

use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Public constants
// ---------------------------------------------------------------------------

/// Number of fenced slots in the KFENCE pool.
pub const KFENCE_POOL_SIZE: usize = 16;

/// Maximum object size that can be placed in a KFENCE slot (bytes).
pub const KFENCE_OBJECT_SIZE: usize = 256;

/// Canary byte written into every redzone byte.
pub const KFENCE_REDZONE_BYTE: u8 = 0xAC;

/// One in every `KFENCE_SAMPLE_INTERVAL` allocations is redirected to KFENCE.
pub const KFENCE_SAMPLE_INTERVAL: u64 = 500;

/// Byte pattern written over freed KFENCE data regions (use-after-free bait).
const KFENCE_FREE_BYTE: u8 = 0xBB;

/// Size of each guard redzone (bytes).
const REDZONE_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Identifies which redzone (left or right) was corrupted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KfenceSide {
    /// The redzone that precedes the allocation data.
    Left,
    /// The redzone that follows the allocation data.
    Right,
}

/// A memory-safety violation detected by KFENCE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KfenceViolation {
    /// A redzone surrounding a KFENCE allocation was overwritten.
    RedzoneCorruption {
        /// Index of the KFENCE slot (0 .. [`KFENCE_POOL_SIZE`]).
        slot: usize,
        /// Which guard redzone was corrupted.
        side: KfenceSide,
    },
    /// The data region of a freed KFENCE slot was modified after the free.
    UseAfterFree {
        /// Index of the KFENCE slot (0 .. [`KFENCE_POOL_SIZE`]).
        slot: usize,
    },
}

/// Cumulative statistics for the KFENCE subsystem.
#[derive(Debug, Clone, Copy, Default)]
pub struct KfenceStats {
    /// Total number of allocations served by KFENCE.
    pub total_allocated: u64,
    /// Total number of KFENCE allocations that were subsequently freed.
    pub total_freed: u64,
    /// Total number of violations detected by [`kfence_check_canaries`] or
    /// [`kfence_report_violation`].
    pub violations_detected: u64,
}

// ---------------------------------------------------------------------------
// Internal slot layout
// ---------------------------------------------------------------------------

/// A single KFENCE slot.
///
/// Each slot contains a left redzone, the data region, and a right redzone.
/// The redzones are initialised to [`KFENCE_REDZONE_BYTE`] on every allocation.
struct KfenceSlot {
    /// Whether this slot currently holds a live allocation.
    allocated: bool,
    /// Whether this slot has been freed (but not yet reused).
    freed: bool,
    /// Number of bytes actually allocated (≤ [`KFENCE_OBJECT_SIZE`]).
    alloc_size: u32,
    /// Left guard redzone.
    left_redzone: [u8; REDZONE_SIZE],
    /// User data region.
    data: [u8; KFENCE_OBJECT_SIZE],
    /// Right guard redzone.
    right_redzone: [u8; REDZONE_SIZE],
}

impl KfenceSlot {
    const fn new() -> Self {
        Self {
            allocated: false,
            freed: false,
            alloc_size: 0,
            left_redzone: [0u8; REDZONE_SIZE],
            data: [0u8; KFENCE_OBJECT_SIZE],
            right_redzone: [0u8; REDZONE_SIZE],
        }
    }

    /// Initialise this slot for a new allocation of `size` bytes.
    ///
    /// Fills both redzones with [`KFENCE_REDZONE_BYTE`] and zeroes the data
    /// region so it is clean for the caller.
    fn init_for_alloc(&mut self, size: usize) {
        self.allocated = true;
        self.freed = false;
        self.alloc_size = size as u32;
        self.left_redzone = [KFENCE_REDZONE_BYTE; REDZONE_SIZE];
        self.right_redzone = [KFENCE_REDZONE_BYTE; REDZONE_SIZE];
        self.data = [0u8; KFENCE_OBJECT_SIZE];
    }

    /// Mark this slot as freed and overwrite the data region with
    /// [`KFENCE_FREE_BYTE`] as use-after-free bait.
    fn mark_freed(&mut self) {
        self.freed = true;
        self.allocated = false;
        // Overwrite the data region so that any subsequent write is detectable.
        self.data = [KFENCE_FREE_BYTE; KFENCE_OBJECT_SIZE];
    }

    /// Return a pointer to the beginning of the data region.
    fn data_ptr(&mut self) -> *mut u8 {
        self.data.as_mut_ptr()
    }

    /// Check the left redzone for corruption.
    fn left_redzone_intact(&self) -> bool {
        self.left_redzone.iter().all(|&b| b == KFENCE_REDZONE_BYTE)
    }

    /// Check the right redzone for corruption.
    fn right_redzone_intact(&self) -> bool {
        self.right_redzone.iter().all(|&b| b == KFENCE_REDZONE_BYTE)
    }

    /// Check whether the freed data region is still untouched (all `0xBB`).
    fn freed_data_intact(&self) -> bool {
        let used = self.alloc_size as usize;
        self.data[..used].iter().all(|&b| b == KFENCE_FREE_BYTE)
    }
}

// ---------------------------------------------------------------------------
// Pool
// ---------------------------------------------------------------------------

/// The complete KFENCE pool — an array of [`KfenceSlot`]s plus statistics.
struct KfencePool {
    slots: [KfenceSlot; KFENCE_POOL_SIZE],
    stats: KfenceStats,
}

impl KfencePool {
    const fn new() -> Self {
        // `KfenceSlot::new()` is `const fn`, so we can build the array in a
        // const context without `Copy`.
        const EMPTY: KfenceSlot = KfenceSlot::new();
        Self {
            slots: [EMPTY; KFENCE_POOL_SIZE],
            stats: KfenceStats {
                total_allocated: 0,
                total_freed: 0,
                violations_detected: 0,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

/// Global KFENCE pool protected by a spinlock.
static POOL: Mutex<KfencePool> = Mutex::new(KfencePool::new());

/// Monotonically increasing counter of calls to [`kfence_alloc`].  Used for
/// sampling: every [`KFENCE_SAMPLE_INTERVAL`]-th call is intercepted.
static ALLOC_COUNTER: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Attempt to allocate `size` bytes from the KFENCE pool.
///
/// Returns `Some(ptr)` pointing at the beginning of the data region when:
/// * The call number is a multiple of [`KFENCE_SAMPLE_INTERVAL`], **and**
/// * `size` ≤ [`KFENCE_OBJECT_SIZE`], **and**
/// * A free slot exists in the pool.
///
/// Returns `None` in all other cases — callers must fall back to their normal
/// allocator.
pub fn kfence_alloc(size: usize) -> Option<*mut u8> {
    let count = ALLOC_COUNTER.fetch_add(1, Ordering::Relaxed);

    // Only redirect every KFENCE_SAMPLE_INTERVAL-th allocation.
    if count % KFENCE_SAMPLE_INTERVAL != 0 {
        return None;
    }

    // Sizes larger than the slot data region cannot be served.
    if size > KFENCE_OBJECT_SIZE || size == 0 {
        return None;
    }

    let mut pool = POOL.lock();

    // Find the first free (never-allocated or recycled) slot.
    let slot_idx = pool
        .slots
        .iter()
        .position(|s| !s.allocated && !s.freed)?;

    pool.slots[slot_idx].init_for_alloc(size);
    pool.stats.total_allocated += 1;

    // SAFETY: `slot_idx` is a valid index into the `slots` array, and
    // `data_ptr()` returns a pointer into the slot's `data` field which is
    // owned by the `KfencePool` that lives for `'static`.  The caller is
    // responsible for using the pointer only while the slot is alive.
    let ptr = pool.slots[slot_idx].data_ptr();
    Some(ptr)
}

/// Attempt to free a KFENCE allocation.
///
/// Returns `true` if `ptr` was a pointer into the KFENCE pool data region and
/// the slot has now been marked as freed.  Returns `false` if `ptr` does not
/// belong to KFENCE (the caller should then use its normal `free` path).
pub fn kfence_free(ptr: *mut u8) -> bool {
    if ptr.is_null() {
        return false;
    }

    let mut pool = POOL.lock();

    for slot in pool.slots.iter_mut() {
        if !slot.allocated {
            continue;
        }

        // SAFETY: `data_ptr()` returns a pointer to `slot.data[0]`, which is
        // the same pointer we handed to the caller in `kfence_alloc`.
        let slot_ptr = slot.data_ptr();
        if slot_ptr == ptr {
            slot.mark_freed();
            pool.stats.total_freed += 1;
            return true;
        }
    }

    false
}

/// Scan all KFENCE slots for redzone corruption and use-after-free writes.
///
/// Returns the number of violations found. Each violation is also reported via
/// [`kfence_report_violation`].
pub fn kfence_check_canaries() -> usize {
    let mut count = 0usize;

    // We need to collect violations first, then report them.  Reporting
    // borrows `pool` through the Mutex so we avoid holding the lock while
    // calling `kfence_report_violation` (which re-acquires it to bump stats).
    let violations = {
        let pool = POOL.lock();
        let mut found: [Option<KfenceViolation>; KFENCE_POOL_SIZE * 2] =
            [None; KFENCE_POOL_SIZE * 2];
        let mut idx = 0usize;

        for (slot_idx, slot) in pool.slots.iter().enumerate() {
            // Check allocated slots for redzone corruption.
            if slot.allocated {
                if !slot.left_redzone_intact() {
                    if idx < found.len() {
                        found[idx] = Some(KfenceViolation::RedzoneCorruption {
                            slot: slot_idx,
                            side: KfenceSide::Left,
                        });
                        idx += 1;
                    }
                }
                if !slot.right_redzone_intact() {
                    if idx < found.len() {
                        found[idx] = Some(KfenceViolation::RedzoneCorruption {
                            slot: slot_idx,
                            side: KfenceSide::Right,
                        });
                        idx += 1;
                    }
                }
            }

            // Check freed slots for use-after-free writes.
            if slot.freed && !slot.freed_data_intact() {
                if idx < found.len() {
                    found[idx] = Some(KfenceViolation::UseAfterFree { slot: slot_idx });
                    idx += 1;
                }
            }
        }
        found
    }; // lock released here

    for v in violations.iter().flatten() {
        kfence_report_violation(*v);
        count += 1;
    }

    count
}

/// Record a KFENCE violation.
///
/// Increments the global violations counter and emits a log message via the
/// kernel serial console.
pub fn kfence_report_violation(v: KfenceViolation) {
    {
        let mut pool = POOL.lock();
        pool.stats.violations_detected += 1;
    }

    match v {
        KfenceViolation::RedzoneCorruption { slot, side } => {
            crate::serial::println!(
                "[KFENCE] BUG: Redzone corruption in slot {} ({:?})",
                slot,
                side
            );
        }
        KfenceViolation::UseAfterFree { slot } => {
            crate::serial::println!("[KFENCE] BUG: Use-after-free in slot {}", slot);
        }
    }
}

/// Return a snapshot of the current KFENCE statistics.
pub fn kfence_stats() -> KfenceStats {
    POOL.lock().stats
}

/// Return `true` if `ptr` points anywhere inside the KFENCE pool data region.
///
/// This is a coarse check — it considers any pointer that happens to alias a
/// KFENCE slot's `data` array as a KFENCE pointer.
pub fn kfence_is_kfence_ptr(ptr: *const u8) -> bool {
    if ptr.is_null() {
        return false;
    }

    let pool = POOL.lock();

    for slot in pool.slots.iter() {
        // SAFETY: we are only computing pointer arithmetic on the `data` field
        // of a slot that lives in the `'static` POOL.  We do not dereference
        // the result; the comparison is purely numeric.
        let start = slot.data.as_ptr();
        // SAFETY: `data` has `KFENCE_OBJECT_SIZE` bytes, so `start + KFENCE_OBJECT_SIZE`
        // is one-past-the-end of the array — a valid computation per the
        // standard pointer arithmetic rules.
        let end = unsafe { start.add(KFENCE_OBJECT_SIZE) };

        if ptr >= start && ptr < end {
            return true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset the pool to a known-clean state between tests so that individual
    /// tests do not interfere with each other.  We also reset the alloc counter
    /// to a known value so that sampling behaviour is predictable.
    fn reset_pool() {
        let mut pool = POOL.lock();
        const EMPTY: KfenceSlot = KfenceSlot::new();
        pool.slots = [EMPTY; KFENCE_POOL_SIZE];
        pool.stats = KfenceStats::default();
        // Set counter so the next kfence_alloc call (count=0) hits the sample.
        ALLOC_COUNTER.store(0, Ordering::SeqCst);
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Force the next call to `kfence_alloc` to be sampled by setting the
    /// counter to `KFENCE_SAMPLE_INTERVAL - 1` (so after `fetch_add(1)` it
    /// equals `KFENCE_SAMPLE_INTERVAL` → 0 mod 500 ... actually set to 0).
    fn prime_counter_for_sample() {
        // After fetch_add(1) the value seen by the function is the *old* value.
        // We want `old % KFENCE_SAMPLE_INTERVAL == 0`, so old must be a
        // multiple of KFENCE_SAMPLE_INTERVAL.  Start at 0.
        ALLOC_COUNTER.store(0, Ordering::SeqCst);
    }

    /// Force the next call to `kfence_alloc` to be NOT sampled.
    fn prime_counter_for_no_sample() {
        ALLOC_COUNTER.store(1, Ordering::SeqCst);
    }

    // -----------------------------------------------------------------------
    // Alloc / free cycle
    // -----------------------------------------------------------------------

    #[test]
    fn test_alloc_returns_some_when_sampled() {
        reset_pool();
        prime_counter_for_sample();
        let ptr = kfence_alloc(64);
        assert!(ptr.is_some(), "expected Some from sampled alloc");
    }

    #[test]
    fn test_alloc_returns_none_when_not_sampled() {
        reset_pool();
        prime_counter_for_no_sample();
        let ptr = kfence_alloc(64);
        assert!(ptr.is_none(), "expected None for non-sampled alloc");
    }

    #[test]
    fn test_alloc_returns_none_for_zero_size() {
        reset_pool();
        prime_counter_for_sample();
        let ptr = kfence_alloc(0);
        assert!(ptr.is_none());
    }

    #[test]
    fn test_alloc_returns_none_for_oversized() {
        reset_pool();
        prime_counter_for_sample();
        let ptr = kfence_alloc(KFENCE_OBJECT_SIZE + 1);
        assert!(ptr.is_none());
    }

    #[test]
    fn test_alloc_exact_max_size_succeeds() {
        reset_pool();
        prime_counter_for_sample();
        let ptr = kfence_alloc(KFENCE_OBJECT_SIZE);
        assert!(ptr.is_some());
    }

    #[test]
    fn test_free_returns_true_for_kfence_ptr() {
        reset_pool();
        prime_counter_for_sample();
        let ptr = kfence_alloc(32).expect("alloc should succeed");
        assert!(kfence_free(ptr), "free should recognise a KFENCE pointer");
    }

    #[test]
    fn test_free_returns_false_for_non_kfence_ptr() {
        reset_pool();
        let stack_val: u8 = 42;
        let foreign_ptr = &stack_val as *const u8 as *mut u8;
        assert!(!kfence_free(foreign_ptr));
    }

    #[test]
    fn test_free_null_returns_false() {
        reset_pool();
        assert!(!kfence_free(core::ptr::null_mut()));
    }

    #[test]
    fn test_alloc_free_cycle_stats() {
        reset_pool();
        prime_counter_for_sample();

        let ptr = kfence_alloc(16).expect("alloc should succeed");
        {
            let s = kfence_stats();
            assert_eq!(s.total_allocated, 1);
            assert_eq!(s.total_freed, 0);
        }

        assert!(kfence_free(ptr));
        {
            let s = kfence_stats();
            assert_eq!(s.total_allocated, 1);
            assert_eq!(s.total_freed, 1);
        }
    }

    // -----------------------------------------------------------------------
    // Use-after-free detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_use_after_free_detected() {
        reset_pool();
        prime_counter_for_sample();

        let ptr = kfence_alloc(32).expect("alloc should succeed");
        assert!(kfence_free(ptr), "free must succeed");

        // Simulate a use-after-free by writing to the freed data region.
        // SAFETY: we intentionally corrupt the freed buffer to verify that
        // `kfence_check_canaries` catches it.  This is only valid in the test
        // context where we own the pool.
        unsafe {
            ptr.write(0xDE);
        }

        let violations = kfence_check_canaries();
        assert!(violations > 0, "expected at least one use-after-free violation");

        let s = kfence_stats();
        assert!(s.violations_detected > 0);
    }

    // -----------------------------------------------------------------------
    // Redzone corruption detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_redzone_corruption_right_detected() {
        reset_pool();
        prime_counter_for_sample();

        let ptr = kfence_alloc(32).expect("alloc should succeed");

        // Corrupt the right redzone by writing past the allocation.
        // SAFETY: We deliberately write into the right redzone to verify that
        // `kfence_check_canaries` catches the corruption.  Only valid in tests.
        unsafe {
            // `ptr` points to `slot.data[0]`.  Writing at offset 32 lands in
            // the data region (past the used bytes) but still within the 256-byte
            // data array.  To actually hit the *right_redzone* we write into
            // the pool slot directly through the pool lock.
            let _ = ptr; // used above
        }

        // Corrupt the redzone via the pool directly.
        {
            let mut pool = POOL.lock();
            for slot in pool.slots.iter_mut() {
                if slot.allocated {
                    slot.right_redzone[0] = 0xFF;
                    break;
                }
            }
        }

        let violations = kfence_check_canaries();
        assert!(violations > 0, "expected right-redzone corruption to be detected");
    }

    #[test]
    fn test_redzone_corruption_left_detected() {
        reset_pool();
        prime_counter_for_sample();

        let _ptr = kfence_alloc(32).expect("alloc should succeed");

        // Corrupt the left redzone directly.
        {
            let mut pool = POOL.lock();
            for slot in pool.slots.iter_mut() {
                if slot.allocated {
                    slot.left_redzone[15] = 0x00;
                    break;
                }
            }
        }

        let violations = kfence_check_canaries();
        assert!(violations > 0, "expected left-redzone corruption to be detected");
    }

    // -----------------------------------------------------------------------
    // Clean canary check — no violations on intact pool
    // -----------------------------------------------------------------------

    #[test]
    fn test_no_violations_on_clean_pool() {
        reset_pool();
        prime_counter_for_sample();
        let _ptr = kfence_alloc(16).expect("alloc should succeed");
        let violations = kfence_check_canaries();
        assert_eq!(violations, 0, "clean pool must report zero violations");
    }

    // -----------------------------------------------------------------------
    // kfence_is_kfence_ptr
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_kfence_ptr_true_for_allocated() {
        reset_pool();
        prime_counter_for_sample();
        let ptr = kfence_alloc(8).expect("alloc should succeed");
        assert!(kfence_is_kfence_ptr(ptr as *const u8));
    }

    #[test]
    fn test_is_kfence_ptr_false_for_foreign() {
        reset_pool();
        let x: u8 = 0;
        assert!(!kfence_is_kfence_ptr(&x as *const u8));
    }

    #[test]
    fn test_is_kfence_ptr_null() {
        reset_pool();
        assert!(!kfence_is_kfence_ptr(core::ptr::null()));
    }

    // -----------------------------------------------------------------------
    // Pool exhaustion
    // -----------------------------------------------------------------------

    #[test]
    fn test_pool_exhaustion_returns_none() {
        reset_pool();

        // Fill all slots.
        for _ in 0..KFENCE_POOL_SIZE {
            prime_counter_for_sample();
            let _ = kfence_alloc(8);
        }

        // One more sampled alloc should fail (pool full).
        prime_counter_for_sample();
        let ptr = kfence_alloc(8);
        assert!(ptr.is_none(), "should return None when pool is exhausted");
    }

    // -----------------------------------------------------------------------
    // Stats tracking
    // -----------------------------------------------------------------------

    #[test]
    fn test_stats_violations_tracked() {
        reset_pool();
        prime_counter_for_sample();

        let ptr = kfence_alloc(32).expect("alloc should succeed");
        kfence_free(ptr);

        // Simulate UAF.
        // SAFETY: intentional corruption for test purposes.
        unsafe { ptr.write(0x00) };

        kfence_check_canaries();

        let s = kfence_stats();
        assert!(s.violations_detected >= 1);
    }

    #[test]
    fn test_stats_default_zero() {
        reset_pool();
        let s = kfence_stats();
        assert_eq!(s.total_allocated, 0);
        assert_eq!(s.total_freed, 0);
        assert_eq!(s.violations_detected, 0);
    }

    // -----------------------------------------------------------------------
    // Violation enum coverage
    // -----------------------------------------------------------------------

    #[test]
    fn test_violation_debug() {
        let v = KfenceViolation::RedzoneCorruption {
            slot: 3,
            side: KfenceSide::Right,
        };
        // Ensure Debug is reachable (no panic).
        let _ = alloc::format!("{:?}", v);
    }

    #[test]
    fn test_kfence_side_debug() {
        let _ = alloc::format!("{:?}", KfenceSide::Left);
        let _ = alloc::format!("{:?}", KfenceSide::Right);
    }
}
