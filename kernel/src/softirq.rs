//! Software interrupt (softirq) processing.
//!
//! Softirqs allow deferring work from hard interrupt context to a safer
//! context.  When a hard interrupt raises a softirq, it is processed
//! after the interrupt returns.
//!
//! # Overview
//!
//! ```ignore
//! use crate::softirq::{Softirq, softirq_process};
//!
//! Softirq::NetTx.raise();
//! softirq_process();
//! ```

use core::sync::atomic::{AtomicU32, Ordering};

/// Number of softirq vectors.
const NUM_SOFTIRQS: usize = 8;

/// Bitmask of pending softirqs.
static PENDING: AtomicU32 = AtomicU32::new(0);

/// Registered softirq handlers.
type HandlerList = [Option<fn()>; NUM_SOFTIRQS];
static HANDLERS: spin::Mutex<HandlerList> = spin::Mutex::new([None; NUM_SOFTIRQS]);

/// Softirq vector identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Softirq {
    /// Timer processing (highest priority).
    Timer = 0,
    /// Network transmit completion.
    NetTx = 1,
    /// Network receive processing.
    NetRx = 2,
    /// Block device completion.
    Block = 3,
    /// Tasklet / workqueue processing.
    Tasklet = 4,
    /// Scheduler housekeeping.
    Scheduler = 5,
    /// Security audit events.
    Security = 6,
    /// Unused / available.
    Unused = 7,
}

impl Softirq {
    /// Convert to a bit index.
    fn bit(self) -> u32 {
        1u32 << (self as u32)
    }

    /// Raise (mark as pending) this softirq.
    pub fn raise(self) {
        PENDING.fetch_or(self.bit(), Ordering::Release);
    }
}

/// Register a handler for a softirq vector.
///
/// Must be called during initialization, before any softirqs are raised.
pub fn softirq_register(irq: Softirq, handler: fn()) {
    let mut handlers = HANDLERS.lock();
    handlers[irq as usize] = Some(handler);
}

/// Process all pending softirqs.
///
/// Called from the timer tick handler or idle loop.
pub fn softirq_process() {
    let pending = PENDING.swap(0, Ordering::Acquire);

    if pending == 0 {
        return;
    }

    let handlers = HANDLERS.lock();

    for i in 0..NUM_SOFTIRQS {
        if pending & (1u32 << i) != 0
            && let Some(handler) = handlers[i]
        {
            handler();
        }
    }
}

/// Check if any softirqs are pending.
pub fn softirq_pending() -> bool {
    PENDING.load(Ordering::Acquire) != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial;
    use core::sync::atomic::AtomicU32;

    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn test_handler() {
        TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    }

    #[test]
    fn softirq_raise_and_process() {
        let _s = test_serial::acquire();
        TEST_COUNTER.store(0, Ordering::Relaxed);
        PENDING.store(0, Ordering::Relaxed);

        softirq_register(Softirq::Timer, test_handler);

        Softirq::Timer.raise();
        assert!(softirq_pending());

        softirq_process();
        assert_eq!(TEST_COUNTER.load(Ordering::Relaxed), 1);
        assert!(!softirq_pending());

        HANDLERS.lock()[Softirq::Timer as usize] = None;
    }

    #[test]
    fn softirq_multiple_vectors() {
        let _s = test_serial::acquire();
        TEST_COUNTER.store(0, Ordering::Relaxed);
        PENDING.store(0, Ordering::Relaxed);

        softirq_register(Softirq::NetTx, test_handler);
        softirq_register(Softirq::Block, test_handler);

        Softirq::NetTx.raise();
        Softirq::Block.raise();

        softirq_process();
        assert_eq!(TEST_COUNTER.load(Ordering::Relaxed), 2);

        let mut handlers = HANDLERS.lock();
        handlers[Softirq::NetTx as usize] = None;
        handlers[Softirq::Block as usize] = None;
    }
}
