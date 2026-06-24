//! Completion — a one-shot synchronisation primitive.
//!
//! One side calls [`Completion::complete`] to signal that some event has
//! occurred; any number of waiters spin on [`Completion::wait`] until that
//! signal arrives.  The primitive can be reset with [`Completion::reset`]
//! for reuse.
//!
//! # Example
//!
//! ```ignore
//! static DONE: Completion = Completion::new();
//!
//! // Producer
//! DONE.complete();
//!
//! // Consumer (spins until the producer fires)
//! DONE.wait();
//! ```

use core::sync::atomic::{AtomicBool, Ordering};

/// A one-shot event: one side signals, the other side waits.
pub struct Completion {
    done: AtomicBool,
}

// SAFETY: `Completion` uses an `AtomicBool` for all shared state, so it is
// safe to share across threads and to send between threads.
unsafe impl Send for Completion {}
unsafe impl Sync for Completion {}

impl Completion {
    /// Create a new, incomplete `Completion`.
    pub const fn new() -> Self {
        Self {
            done: AtomicBool::new(false),
        }
    }

    /// Mark this completion as done.
    ///
    /// Any threads currently spinning in [`wait`](Self::wait) will return
    /// as soon as they observe the updated state.
    pub fn complete(&self) {
        self.done.store(true, Ordering::Release);
    }

    /// Spin until the completion has been signalled.
    ///
    /// Uses [`core::hint::spin_loop`] to yield the CPU pipeline hint on
    /// every iteration.
    pub fn wait(&self) {
        while !self.done.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
    }

    /// Spin for at most `max_spins` iterations waiting for the completion.
    ///
    /// Returns `true` if the completion was observed within the spin budget,
    /// or `false` if the timeout was reached.
    pub fn wait_timeout(&self, max_spins: u64) -> bool {
        for _ in 0..max_spins {
            if self.done.load(Ordering::Acquire) {
                return true;
            }
            core::hint::spin_loop();
        }
        // One final check after exhausting the budget.
        self.done.load(Ordering::Acquire)
    }

    /// Non-blocking check: returns `true` if the completion has been signalled.
    pub fn is_complete(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    /// Reset this completion to the incomplete state so it can be reused.
    pub fn reset(&self) {
        self.done.store(false, Ordering::Release);
    }
}

impl Default for Completion {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial;

    #[test]
    fn completion_starts_incomplete() {
        let _s = test_serial::acquire();
        let c = Completion::new();
        assert!(!c.is_complete());
    }

    #[test]
    fn completion_complete_makes_is_complete_true() {
        let _s = test_serial::acquire();
        let c = Completion::new();
        c.complete();
        assert!(c.is_complete());
    }

    #[test]
    fn completion_wait_returns_immediately_when_already_done() {
        let _s = test_serial::acquire();
        let c = Completion::new();
        c.complete();
        // Should return without spinning indefinitely.
        c.wait();
    }

    #[test]
    fn completion_wait_timeout_succeeds_when_already_done() {
        let _s = test_serial::acquire();
        let c = Completion::new();
        c.complete();
        assert!(c.wait_timeout(1_000));
    }

    #[test]
    fn completion_wait_timeout_fails_when_never_signalled() {
        let _s = test_serial::acquire();
        let c = Completion::new();
        // Never call complete() — timeout must fire.
        assert!(!c.wait_timeout(100));
    }

    #[test]
    fn completion_reset_clears_done_flag() {
        let _s = test_serial::acquire();
        let c = Completion::new();
        c.complete();
        assert!(c.is_complete());
        c.reset();
        assert!(!c.is_complete());
    }

    #[test]
    fn completion_reuse_after_reset() {
        let _s = test_serial::acquire();
        let c = Completion::new();

        // First use.
        c.complete();
        assert!(c.wait_timeout(1_000));

        // Reset and reuse.
        c.reset();
        assert!(!c.is_complete());
        c.complete();
        assert!(c.is_complete());
    }

    #[test]
    fn completion_default_is_incomplete() {
        let _s = test_serial::acquire();
        let c = Completion::default();
        assert!(!c.is_complete());
    }
}
