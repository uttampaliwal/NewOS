//! timerfd — timer-based file descriptor notification.
//!
//! A timerfd creates a file descriptor that becomes readable when a
//! timer expires.  It supports one-shot and periodic timers.
//!
//! # Timer Modes
//!
//! - **One-shot**: fires once, then disarms.
//! - **Periodic**: fires repeatedly at the specified interval.

use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use crate::task::TaskId;
use crate::task::scheduler::{block_current, get_current_task_id, wake_task_by_id};

/// Timer flags.
pub const TFD_TIMER_ABSTIME: u32 = 1;

/// A timerfd instance.
pub struct TimerFd {
    /// Whether the timer is armed.
    armed: AtomicBool,
    /// Interval in ticks (0 = one-shot).
    interval_ticks: AtomicU64,
    /// Absolute expiration in ticks.
    expiration: AtomicU64,
    /// Number of expirations since last read.
    expirations: AtomicU64,
    /// Tasks waiting for the timer to fire.
    waiters: Mutex<VecDeque<TaskId>>,
}

impl TimerFd {
    /// Create a new disarmed timerfd.
    pub fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
            interval_ticks: AtomicU64::new(0),
            expiration: AtomicU64::new(0),
            expirations: AtomicU64::new(0),
            waiters: Mutex::new(VecDeque::new()),
        }
    }

    /// Set (arm) the timer.
    ///
    /// - `initial_ticks`: time until first expiration (or absolute time
    ///   if `TFD_TIMER_ABSTIME` is set).
    /// - `interval_ticks`: interval for periodic timers (0 = one-shot).
    /// - `flags`: `TFD_TIMER_ABSTIME` for absolute time.
    pub fn settime(&self, initial_ticks: u64, interval_ticks: u64, flags: u32) {
        let now = crate::time::uptime_ticks();

        let new_exp = if flags & TFD_TIMER_ABSTIME != 0 {
            initial_ticks
        } else {
            now.saturating_add(initial_ticks)
        };

        self.expiration.store(new_exp, Ordering::Release);
        self.interval_ticks.store(interval_ticks, Ordering::Release);
        self.armed.store(true, Ordering::Release);
    }

    /// Get the current timer settings.
    ///
    /// Returns `(remaining_ticks, interval_ticks)`.
    pub fn gettime(&self) -> (u64, u64) {
        if !self.armed.load(Ordering::Acquire) {
            return (0, self.interval_ticks.load(Ordering::Relaxed));
        }
        let now = crate::time::uptime_ticks();
        let exp = self.expiration.load(Ordering::Relaxed);
        let remaining = exp.saturating_sub(now);
        (remaining, self.interval_ticks.load(Ordering::Relaxed))
    }

    /// Called by the timer tick to check if the timer has expired.
    /// Returns true if it fired.
    pub fn tick(&self) -> bool {
        if !self.armed.load(Ordering::Acquire) {
            return false;
        }

        let now = crate::time::uptime_ticks();
        let exp = self.expiration.load(Ordering::Acquire);

        if now >= exp {
            self.expirations.fetch_add(1, Ordering::Release);

            // Wake waiters.
            let mut waiters = self.waiters.lock();
            while let Some(task_id) = waiters.pop_front() {
                wake_task_by_id(task_id);
            }

            // Re-arm if periodic.
            let interval = self.interval_ticks.load(Ordering::Relaxed);
            if interval > 0 {
                self.expiration.store(now + interval, Ordering::Release);
            } else {
                self.armed.store(false, Ordering::Release);
            }

            return true;
        }

        false
    }

    /// Read the number of expirations since last read.
    #[allow(clippy::result_unit_err)]
    pub fn read_expirations(&self) -> Result<u64, ()> {
        loop {
            let exps = self.expirations.load(Ordering::Acquire);
            if exps > 0 {
                if self
                    .expirations
                    .compare_exchange(exps, 0, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    return Ok(exps);
                }
                continue;
            }

            if !self.armed.load(Ordering::Acquire) {
                return Err(());
            }

            // Block until timer fires.
            if let Some(tid) = get_current_task_id() {
                self.waiters.lock().push_back(tid);
                block_current();
            }
        }
    }

    /// Poll for readability (timer has fired).
    pub fn poll(&self) -> u8 {
        if self.expirations.load(Ordering::Acquire) > 0 {
            0x01 // EPOLLIN
        } else {
            0
        }
    }
}

impl Default for TimerFd {
    fn default() -> Self {
        Self::new()
    }
}

/// Global list of active timerfds for tick processing.
static TIMER_FDS: Mutex<VecDeque<alloc::sync::Arc<TimerFd>>> = Mutex::new(VecDeque::new());

/// Register a timerfd for tick processing.
pub fn register_timerfd(tfd: alloc::sync::Arc<TimerFd>) {
    TIMER_FDS.lock().push_back(tfd);
}

/// Called from the timer tick handler to fire expired timerfds.
pub fn timerfd_tick() {
    let fds = TIMER_FDS.lock();
    for tfd in fds.iter() {
        tfd.tick();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial;

    #[test]
    fn timerfd_new_disarmed() {
        let _s = test_serial::acquire();
        let tfd = TimerFd::new();
        assert!(!tfd.armed.load(Ordering::Relaxed));
        assert_eq!(tfd.peek_expirations(), 0);
    }

    impl TimerFd {
        fn peek_expirations(&self) -> u64 {
            self.expirations.load(Ordering::Relaxed)
        }
    }

    #[test]
    fn timerfd_settime_one_shot() {
        let _s = test_serial::acquire();
        let tfd = TimerFd::new();
        tfd.settime(100, 0, 0);
        assert!(tfd.armed.load(Ordering::Relaxed));
        let (remaining, interval) = tfd.gettime();
        assert!(remaining > 0);
        assert_eq!(interval, 0);
    }

    #[test]
    fn timerfd_settime_periodic() {
        let _s = test_serial::acquire();
        let tfd = TimerFd::new();
        tfd.settime(100, 50, 0);
        assert!(tfd.armed.load(Ordering::Relaxed));
        let (_remaining, interval) = tfd.gettime();
        assert_eq!(interval, 50);
    }
}
