//! Hardware watchdog timer with pre-panic countdown.
//!
//! The watchdog monitors system liveness. A periodic "kick" (pet) resets the
//! counter. If the counter reaches zero, the system is considered hung and
//! enters a pre-panic state with a configurable countdown before panicking.

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

struct WatchdogState {
    armed: bool,
    timeout_ticks: u64,
    countdown: u64,
    kick_count: u64,
    fire_count: u64,
    in_panic: bool,
    pre_panic_ticks: u64,
    pre_panic_countdown: u64,
}

static WATCHDOG: Mutex<WatchdogState> = Mutex::new(WatchdogState {
    armed: false,
    timeout_ticks: 0,
    countdown: 0,
    kick_count: 0,
    fire_count: 0,
    in_panic: false,
    pre_panic_ticks: 0,
    pre_panic_countdown: 0,
});

static PANIC_ON_EXPIRY: AtomicBool = AtomicBool::new(true);

pub fn init_watchdog(timeout_ticks: u64, pre_panic_ticks: u64) {
    let mut wd = WATCHDOG.lock();
    wd.armed = true;
    wd.timeout_ticks = timeout_ticks;
    wd.countdown = timeout_ticks;
    wd.kick_count = 0;
    wd.fire_count = 0;
    wd.in_panic = false;
    wd.pre_panic_ticks = pre_panic_ticks;
    wd.pre_panic_countdown = pre_panic_ticks;
}

pub fn kick_watchdog() {
    let mut wd = WATCHDOG.lock();
    if wd.armed && !wd.in_panic {
        wd.countdown = wd.timeout_ticks;
        wd.kick_count += 1;
    }
}

pub fn disarm_watchdog() {
    WATCHDOG.lock().armed = false;
}

pub fn arm_watchdog() {
    let mut wd = WATCHDOG.lock();
    wd.armed = true;
    wd.countdown = wd.timeout_ticks;
}

/// Called by the timer interrupt handler on each tick.
/// Returns `true` if the watchdog has fully expired (panic should follow).
pub fn watchdog_tick() -> bool {
    let mut wd = WATCHDOG.lock();
    if !wd.armed {
        return false;
    }

    if wd.in_panic {
        if wd.pre_panic_countdown > 0 {
            wd.pre_panic_countdown -= 1;
        }
        if wd.pre_panic_countdown == 0 {
            wd.fire_count += 1;
            if PANIC_ON_EXPIRY.load(Ordering::Relaxed) {
                drop(wd);
                panic!("watchdog: pre-panic countdown expired");
            }
            return true;
        }
        return false;
    }

    if wd.countdown > 0 {
        wd.countdown -= 1;
    }

    if wd.countdown == 0 {
        wd.in_panic = true;
        wd.pre_panic_countdown = wd.pre_panic_ticks;
        wd.fire_count += 1;
        drop(wd);
        crate::serial::print(format_args!(
            "WATCHDOG: system hung, entering pre-panic countdown\n"
        ));
        return false;
    }

    false
}

pub fn is_watchdog_armed() -> bool {
    WATCHDOG.lock().armed
}

pub fn is_watchdog_in_panic() -> bool {
    WATCHDOG.lock().in_panic
}

pub fn watchdog_countdown() -> u64 {
    WATCHDOG.lock().countdown
}

pub fn watchdog_kick_count() -> u64 {
    WATCHDOG.lock().kick_count
}

pub fn watchdog_fire_count() -> u64 {
    WATCHDOG.lock().fire_count
}

pub fn reset_watchdog() {
    *WATCHDOG.lock() = WatchdogState {
        armed: false,
        timeout_ticks: 0,
        countdown: 0,
        kick_count: 0,
        fire_count: 0,
        in_panic: false,
        pre_panic_ticks: 0,
        pre_panic_countdown: 0,
    };
    PANIC_ON_EXPIRY.store(true, Ordering::Relaxed);
}

pub fn set_watchdog_panic_on_expiry(panic: bool) {
    PANIC_ON_EXPIRY.store(panic, Ordering::Relaxed);
}

#[derive(Debug, Clone)]
pub struct WatchdogStats {
    pub armed: bool,
    pub timeout_ticks: u64,
    pub countdown: u64,
    pub kick_count: u64,
    pub fire_count: u64,
    pub in_panic: bool,
}

pub fn watchdog_stats() -> WatchdogStats {
    let wd = WATCHDOG.lock();
    WatchdogStats {
        armed: wd.armed,
        timeout_ticks: wd.timeout_ticks,
        countdown: wd.countdown,
        kick_count: wd.kick_count,
        fire_count: wd.fire_count,
        in_panic: wd.in_panic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_disarmed() {
        let _s = crate::test_serial::acquire();
        reset_watchdog();
        assert!(!is_watchdog_armed());
        assert_eq!(watchdog_countdown(), 0);
    }

    #[test]
    fn init_arms() {
        let _s = crate::test_serial::acquire();
        reset_watchdog();
        set_watchdog_panic_on_expiry(false);
        init_watchdog(10, 5);
        assert!(is_watchdog_armed());
        assert_eq!(watchdog_countdown(), 10);
    }

    #[test]
    fn kick_resets_countdown() {
        let _s = crate::test_serial::acquire();
        reset_watchdog();
        set_watchdog_panic_on_expiry(false);
        init_watchdog(10, 5);
        watchdog_tick();
        watchdog_tick();
        assert_eq!(watchdog_countdown(), 8);
        kick_watchdog();
        assert_eq!(watchdog_countdown(), 10);
    }

    #[test]
    fn tick_decrements() {
        let _s = crate::test_serial::acquire();
        reset_watchdog();
        set_watchdog_panic_on_expiry(false);
        init_watchdog(5, 2);
        for _ in 0..4 {
            assert!(!watchdog_tick());
        }
        assert_eq!(watchdog_countdown(), 1);
    }

    #[test]
    fn expiry_triggers_panic_state() {
        let _s = crate::test_serial::acquire();
        reset_watchdog();
        set_watchdog_panic_on_expiry(false);
        init_watchdog(3, 2);
        assert!(!watchdog_tick());
        assert!(!watchdog_tick());
        assert!(!watchdog_tick());
        assert!(is_watchdog_in_panic());
        assert_eq!(watchdog_fire_count(), 1);
    }

    #[test]
    fn pre_panic_countdown_expires() {
        let _s = crate::test_serial::acquire();
        reset_watchdog();
        set_watchdog_panic_on_expiry(false);
        init_watchdog(2, 2);
        watchdog_tick();
        watchdog_tick();
        assert!(is_watchdog_in_panic());
        assert!(!watchdog_tick());
        assert!(watchdog_tick());
    }

    #[test]
    fn disarm_prevents_tick() {
        let _s = crate::test_serial::acquire();
        reset_watchdog();
        set_watchdog_panic_on_expiry(false);
        init_watchdog(3, 1);
        disarm_watchdog();
        for _ in 0..10 {
            assert!(!watchdog_tick());
        }
    }

    #[test]
    fn arm_rearms() {
        let _s = crate::test_serial::acquire();
        reset_watchdog();
        set_watchdog_panic_on_expiry(false);
        init_watchdog(5, 1);
        disarm_watchdog();
        arm_watchdog();
        assert!(is_watchdog_armed());
        assert_eq!(watchdog_countdown(), 5);
    }

    #[test]
    fn kick_count_increments() {
        let _s = crate::test_serial::acquire();
        reset_watchdog();
        set_watchdog_panic_on_expiry(false);
        init_watchdog(10, 1);
        kick_watchdog();
        kick_watchdog();
        kick_watchdog();
        assert_eq!(watchdog_kick_count(), 3);
    }

    #[test]
    fn stats_snapshot() {
        let _s = crate::test_serial::acquire();
        reset_watchdog();
        set_watchdog_panic_on_expiry(false);
        init_watchdog(10, 3);
        kick_watchdog();
        let stats = watchdog_stats();
        assert!(stats.armed);
        assert_eq!(stats.timeout_ticks, 10);
        assert_eq!(stats.kick_count, 1);
    }

    #[test]
    fn zero_timeout_immediate_expiry() {
        let _s = crate::test_serial::acquire();
        reset_watchdog();
        set_watchdog_panic_on_expiry(false);
        init_watchdog(0, 1);
        assert!(!watchdog_tick());
        assert!(is_watchdog_in_panic());
    }

    #[test]
    fn kick_during_panic_no_effect() {
        let _s = crate::test_serial::acquire();
        reset_watchdog();
        set_watchdog_panic_on_expiry(false);
        init_watchdog(1, 1);
        watchdog_tick();
        assert!(is_watchdog_in_panic());
        kick_watchdog();
        assert!(is_watchdog_in_panic());
        assert_eq!(watchdog_countdown(), 0);
    }

    #[test]
    fn long_timeout() {
        let _s = crate::test_serial::acquire();
        reset_watchdog();
        set_watchdog_panic_on_expiry(false);
        init_watchdog(1000, 100);
        for _ in 0..999 {
            assert!(!watchdog_tick());
        }
        assert_eq!(watchdog_countdown(), 1);
        kick_watchdog();
        assert_eq!(watchdog_countdown(), 1000);
    }
}
