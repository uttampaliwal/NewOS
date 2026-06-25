//! Lock dependency tracker — detects potential deadlock ordering violations.
//!
//! `lockdep` maintains a per-"CPU" (simulated via a global array in test
//! contexts) stack of currently-held locks and a global table of observed
//! acquisition orderings.  When a lock is acquired while others are already
//! held, every (held → new) pair is recorded.  If the inverse pair
//! (new → held) has been seen in a previous execution, a warning is emitted
//! because that ordering could produce a deadlock on two CPUs.
//!
//! # Usage
//!
//! ```ignore
//! lock_class!(LOCK_A);
//! lock_class!(LOCK_B);
//!
//! let _ga = lockdep_acquire(&LOCK_A);
//! let _gb = lockdep_acquire(&LOCK_B);
//! // _ga and _gb are dropped in LIFO order automatically.
//! ```

use core::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// LockClass
// ---------------------------------------------------------------------------

/// A static descriptor for one logical lock class (e.g. `SPINLOCK_SLAB`).
///
/// Every distinct lock *type* that should be tracked by lockdep gets its own
/// `LockClass`.  Create one with the [`lock_class!`] macro.
pub struct LockClass {
    /// Human-readable name, shown in warning messages.
    pub name: &'static str,
    /// Unique numeric identifier assigned at compile time via the macro.
    pub id: usize,
}

/// Create a static [`LockClass`] with the given identifier name.
///
/// # Example
///
/// ```ignore
/// lock_class!(MY_MUTEX);
/// ```
#[macro_export]
macro_rules! lock_class {
    ($name:ident) => {
        static $name: $crate::sync::lockdep::LockClass = $crate::sync::lockdep::LockClass {
            name: stringify!($name),
            id: line!() as usize,
        };
    };
}

// ---------------------------------------------------------------------------
// Global ordering table
// ---------------------------------------------------------------------------

/// Maximum number of (first, then) ordering pairs we remember.
const MAX_PAIRS: usize = 64;

/// One recorded ordering: "first was held when then was acquired."
#[derive(Clone, Copy)]
struct OrderPair {
    first: usize,
    then: usize,
}

/// The global table of observed lock acquisition orderings.
struct OrderTable {
    pairs: [OrderPair; MAX_PAIRS],
    len: usize,
}

impl OrderTable {
    const fn new() -> Self {
        Self {
            pairs: [OrderPair { first: 0, then: 0 }; MAX_PAIRS],
            len: 0,
        }
    }

    /// Returns `true` if the (first, then) pair is already recorded.
    fn contains(&self, first: usize, then: usize) -> bool {
        let mut i = 0;
        while i < self.len {
            let p = self.pairs[i];
            if p.first == first && p.then == then {
                return true;
            }
            i += 1;
        }
        false
    }

    /// Record (first, then) if not already present and there is capacity.
    fn insert(&mut self, first: usize, then: usize) {
        if self.len < MAX_PAIRS && !self.contains(first, then) {
            self.pairs[self.len] = OrderPair { first, then };
            self.len += 1;
        }
    }

    fn reset(&mut self) {
        self.len = 0;
    }
}

// ---------------------------------------------------------------------------
// Per-CPU held-lock stack
// ---------------------------------------------------------------------------

/// Maximum lock nesting depth per CPU.
const MAX_DEPTH: usize = 8;

/// One per-CPU slot: the stack of currently-held lock class IDs.
struct CpuStack {
    stack: [usize; MAX_DEPTH],
    depth: usize,
}

impl CpuStack {
    const fn new() -> Self {
        Self {
            stack: [0; MAX_DEPTH],
            depth: 0,
        }
    }

    fn push(&mut self, id: usize) {
        if self.depth < MAX_DEPTH {
            self.stack[self.depth] = id;
            self.depth += 1;
        }
        // Silently ignore overflow — production code would panic or log.
    }

    fn pop(&mut self, id: usize) {
        // Walk from top down to find the matching entry (handles unusual
        // drop orders gracefully).
        let mut i = self.depth;
        while i > 0 {
            i -= 1;
            if self.stack[i] == id {
                // Shift everything above it down by one.
                let mut j = i;
                while j + 1 < self.depth {
                    self.stack[j] = self.stack[j + 1];
                    j += 1;
                }
                self.depth -= 1;
                return;
            }
        }
    }

    fn is_held(&self, id: usize) -> bool {
        let mut i = 0;
        while i < self.depth {
            if self.stack[i] == id {
                return true;
            }
            i += 1;
        }
        false
    }

    fn reset(&mut self) {
        self.depth = 0;
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

// We simulate a single CPU (index 0) for the test context.  In a real SMP
// kernel this would be indexed by `cpu_id()`.
const NUM_CPUS: usize = 1;

/// Maximum number of lock classes we track.
const MAX_LOCK_CLASSES: usize = 64;

/// Registry mapping lock id → name for warning messages.
struct LockRegistry {
    ids: [usize; MAX_LOCK_CLASSES],
    names: [&'static str; MAX_LOCK_CLASSES],
    len: usize,
}

impl LockRegistry {
    const fn new() -> Self {
        Self {
            ids: [0; MAX_LOCK_CLASSES],
            names: [""; MAX_LOCK_CLASSES],
            len: 0,
        }
    }

    fn register(&mut self, id: usize, name: &'static str) {
        if self.len < MAX_LOCK_CLASSES && !self.ids[..self.len].contains(&id) {
            self.ids[self.len] = id;
            self.names[self.len] = name;
            self.len += 1;
        }
    }

    fn get_name(&self, id: usize) -> &'static str {
        for i in 0..self.len {
            if self.ids[i] == id {
                return self.names[i];
            }
        }
        "unknown-lock"
    }
}

/// Interior-mutable wrapper so we can hold mutable state behind a
/// `static`.  Protected by `STATE_LOCK`.
struct LockDepState {
    order_table: OrderTable,
    cpu_stacks: [CpuStack; NUM_CPUS],
    registry: LockRegistry,
}

impl LockDepState {
    const fn new() -> Self {
        Self {
            order_table: OrderTable::new(),
            cpu_stacks: [CpuStack::new()],
            registry: LockRegistry::new(),
        }
    }
}

// SAFETY: All mutable access goes through `STATE_LOCK`.
unsafe impl Send for LockDepState {}
unsafe impl Sync for LockDepState {}

use core::cell::UnsafeCell;

/// Trivial spinlock wrapping `LockDepState`.
struct StateLock {
    locked: AtomicUsize,
    data: UnsafeCell<LockDepState>,
}

// SAFETY: `StateLock` serialises all access via `locked`.
unsafe impl Send for StateLock {}
unsafe impl Sync for StateLock {}

impl StateLock {
    const fn new() -> Self {
        Self {
            locked: AtomicUsize::new(0),
            data: UnsafeCell::new(LockDepState::new()),
        }
    }

    fn lock(&self) -> StateLockGuard<'_> {
        while self
            .locked
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        StateLockGuard { lock: self }
    }
}

struct StateLockGuard<'a> {
    lock: &'a StateLock,
}

impl<'a> StateLockGuard<'a> {
    fn get_mut(&mut self) -> &mut LockDepState {
        // SAFETY: we hold the spinlock.
        unsafe { &mut *self.lock.data.get() }
    }
}

impl Drop for StateLockGuard<'_> {
    fn drop(&mut self) {
        self.lock.locked.store(0, Ordering::Release);
    }
}

static STATE: StateLock = StateLock::new();

// ---------------------------------------------------------------------------
// Warning sink
// ---------------------------------------------------------------------------

/// Violation counter — incremented each time a cycle is detected.
/// Exposed so tests can assert on it without needing `std::io` capture.
static VIOLATION_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Returns the number of ordering violations detected since the last
/// [`lockdep_reset`].
pub fn lockdep_violation_count() -> usize {
    VIOLATION_COUNT.load(Ordering::Relaxed)
}

fn emit_warning(held_name: &'static str, new_name: &'static str) {
    VIOLATION_COUNT.fetch_add(1, Ordering::Relaxed);
    crate::serial::println!(
        "[LOCKDEP] Potential deadlock: holding '{}' while acquiring '{}'",
        held_name,
        new_name
    );
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Guard returned by [`lockdep_acquire`].  Pops the lock from the held-lock
/// stack when dropped.
pub struct LockDepGuard {
    class: &'static LockClass,
}

impl Drop for LockDepGuard {
    fn drop(&mut self) {
        let mut g = STATE.lock();
        let state = g.get_mut();
        state.cpu_stacks[0].pop(self.class.id);
    }
}

/// Acquire `class` in the lockdep tracker.
///
/// If acquiring `class` would violate a previously-observed ordering (i.e.
/// we've recorded that `class` was held *before* something that is currently
/// held), a warning is emitted and the violation counter is incremented.
///
/// Returns a [`LockDepGuard`] that pops `class` from the held-lock stack on
/// drop.
pub fn lockdep_acquire(class: &'static LockClass) -> LockDepGuard {
    let mut g = STATE.lock();
    let state = g.get_mut();
    let cpu = &mut state.cpu_stacks[0];

    // Register this lock class in the name registry
    state.registry.register(class.id, class.name);

    // Check every currently-held lock H:
    //   - Record (H → class) in the ordering table.
    //   - If (class → H) is already in the table, we have a cycle.
    for i in 0..cpu.depth {
        let held_id = cpu.stack[i];

        // Look up the name of the held class for the warning message.
        let held_name = state.registry.get_name(held_id);

        // Check for inverse ordering (potential deadlock).
        if state.order_table.contains(class.id, held_id) {
            emit_warning(held_name, class.name);
        }

        // Record this ordering.
        state.order_table.insert(held_id, class.id);
    }

    cpu.push(class.id);
    LockDepGuard { class }
}

/// Returns `true` if `class` is currently held (on the simulated CPU 0
/// stack).
pub fn lockdep_is_held(class: &'static LockClass) -> bool {
    let mut g = STATE.lock();
    let state = g.get_mut();
    state.cpu_stacks[0].is_held(class.id)
}

/// Reset all lockdep state — ordering table, held-lock stacks, and the
/// violation counter.  Call this between tests.
pub fn lockdep_reset() {
    VIOLATION_COUNT.store(0, Ordering::Relaxed);
    let mut g = STATE.lock();
    let state = g.get_mut();
    state.order_table.reset();
    for stack in state.cpu_stacks.iter_mut() {
        stack.reset();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial;

    // Declare two lock classes for use in tests.
    lock_class!(LOCK_A);
    lock_class!(LOCK_B);
    lock_class!(LOCK_C);

    #[test]
    fn lockdep_acquire_and_is_held() {
        let _s = test_serial::acquire();
        lockdep_reset();

        assert!(!lockdep_is_held(&LOCK_A));
        let _g = lockdep_acquire(&LOCK_A);
        assert!(lockdep_is_held(&LOCK_A));
    }

    #[test]
    fn lockdep_guard_drop_releases() {
        let _s = test_serial::acquire();
        lockdep_reset();

        {
            let _g = lockdep_acquire(&LOCK_A);
            assert!(lockdep_is_held(&LOCK_A));
        }
        assert!(!lockdep_is_held(&LOCK_A));
    }

    #[test]
    fn lockdep_no_violation_on_first_acquisition() {
        let _s = test_serial::acquire();
        lockdep_reset();

        // A then B — first time, no prior ordering recorded.
        let _ga = lockdep_acquire(&LOCK_A);
        let _gb = lockdep_acquire(&LOCK_B);
        assert_eq!(lockdep_violation_count(), 0);
    }

    #[test]
    fn lockdep_detects_ordering_violation() {
        let _s = test_serial::acquire();
        lockdep_reset();

        // Round 1: acquire A then B → records (A → B).
        {
            let _ga = lockdep_acquire(&LOCK_A);
            let _gb = lockdep_acquire(&LOCK_B);
        }
        assert_eq!(lockdep_violation_count(), 0);

        // Round 2: acquire B then A → (B → A) violates recorded (A → B).
        {
            let _gb = lockdep_acquire(&LOCK_B);
            let _ga = lockdep_acquire(&LOCK_A);
        }
        assert!(lockdep_violation_count() > 0);
    }

    #[test]
    fn lockdep_reset_clears_violations() {
        let _s = test_serial::acquire();
        lockdep_reset();

        {
            let _ga = lockdep_acquire(&LOCK_A);
            let _gb = lockdep_acquire(&LOCK_B);
        }
        {
            let _gb = lockdep_acquire(&LOCK_B);
            let _ga = lockdep_acquire(&LOCK_A);
        }
        assert!(lockdep_violation_count() > 0);

        lockdep_reset();
        assert_eq!(lockdep_violation_count(), 0);
    }

    #[test]
    fn lockdep_three_locks_chain() {
        let _s = test_serial::acquire();
        lockdep_reset();

        // A → B → C — no violations.
        let _ga = lockdep_acquire(&LOCK_A);
        let _gb = lockdep_acquire(&LOCK_B);
        let _gc = lockdep_acquire(&LOCK_C);
        assert_eq!(lockdep_violation_count(), 0);
    }

    #[test]
    fn lockdep_is_held_false_after_all_released() {
        let _s = test_serial::acquire();
        lockdep_reset();

        {
            let _ga = lockdep_acquire(&LOCK_A);
            let _gb = lockdep_acquire(&LOCK_B);
        }
        assert!(!lockdep_is_held(&LOCK_A));
        assert!(!lockdep_is_held(&LOCK_B));
    }

    #[test]
    fn lockdep_same_ordering_repeated_no_extra_violations() {
        let _s = test_serial::acquire();
        lockdep_reset();

        // Repeat A → B many times — still no violation.
        for _ in 0..10 {
            let _ga = lockdep_acquire(&LOCK_A);
            let _gb = lockdep_acquire(&LOCK_B);
        }
        assert_eq!(lockdep_violation_count(), 0);
    }
}
