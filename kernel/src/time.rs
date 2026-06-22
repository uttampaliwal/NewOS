//! Kernel time utilities.
//!
//! Provides a monotonic clock based on the scheduler tick counter.
//! Assumes a 1 kHz tick rate (1 ms per tick).

/// Convert scheduler ticks to microseconds.
///
/// The scheduler increments `UPTIME_TICKS` once per timer interrupt.
/// With a 1 kHz tick rate, 1 tick = 1000 µs.
pub fn uptime_us() -> u64 {
    crate::task::scheduler::get_uptime_ticks() * 1000
}
