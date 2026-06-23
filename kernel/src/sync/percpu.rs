//! Per-CPU data framework.
//!
//! Provides `PerCpu<T>` for data replicated per CPU, and `PerCpuCounter`
//! for lock-free per-CPU counters.
//!
//! # Example
//!
//! ```ignore
//! use crate::sync::percpu::PerCpuCounter;
//!
//! static COUNTER: PerCpuCounter = PerCpuCounter::new();
//!
//! COUNTER.inc(0); // CPU 0
//! COUNTER.inc(1); // CPU 1
//! assert_eq!(COUNTER.sum_all(), 2);
//! ```

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

/// Maximum number of CPUs supported.
pub const MAX_CPUS: usize = 256;

/// A per-CPU counter using atomics for lock-free updates.
///
/// This is the preferred per-CPU primitive for simple counters.
pub struct PerCpuCounter {
    values: [AtomicUsize; MAX_CPUS],
}

impl PerCpuCounter {
    /// Create a new per-CPU counter initialized to zero.
    pub fn new() -> Self {
        Self {
            values: core::array::from_fn(|_| AtomicUsize::new(0)),
        }
    }

    /// Increment the counter for a specific CPU.
    pub fn inc(&self, cpu_id: usize) {
        debug_assert!(cpu_id < MAX_CPUS);
        self.values[cpu_id].fetch_add(1, Ordering::Relaxed);
    }

    /// Add a value to the counter for a specific CPU.
    pub fn add(&self, cpu_id: usize, val: usize) {
        debug_assert!(cpu_id < MAX_CPUS);
        self.values[cpu_id].fetch_add(val, Ordering::Relaxed);
    }

    /// Read the counter for a specific CPU.
    pub fn get(&self, cpu_id: usize) -> usize {
        debug_assert!(cpu_id < MAX_CPUS);
        self.values[cpu_id].load(Ordering::Relaxed)
    }

    /// Sum all CPU counters.
    pub fn sum_all(&self) -> usize {
        self.values
            .iter()
            .map(|v| v.load(Ordering::Relaxed))
            .sum()
    }

    /// Reset all CPU counters to zero.
    pub fn reset_all(&self) {
        for v in self.values.iter() {
            v.store(0, Ordering::Relaxed);
        }
    }
}

impl Default for PerCpuCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// A per-CPU atomic u64 counter.
pub struct PerCpuAtomicCounter {
    values: [AtomicU64; MAX_CPUS],
}

impl PerCpuAtomicCounter {
    /// Create a new per-CPU atomic counter initialized to zero.
    pub fn new() -> Self {
        Self {
            values: core::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    /// Increment the counter for a specific CPU.
    pub fn inc(&self, cpu_id: usize) {
        debug_assert!(cpu_id < MAX_CPUS);
        self.values[cpu_id].fetch_add(1, Ordering::Relaxed);
    }

    /// Add a value to the counter for a specific CPU.
    pub fn add(&self, cpu_id: usize, val: u64) {
        debug_assert!(cpu_id < MAX_CPUS);
        self.values[cpu_id].fetch_add(val, Ordering::Relaxed);
    }

    /// Read the counter for a specific CPU.
    pub fn get(&self, cpu_id: usize) -> u64 {
        debug_assert!(cpu_id < MAX_CPUS);
        self.values[cpu_id].load(Ordering::Relaxed)
    }

    /// Sum all CPU counters.
    pub fn sum_all(&self) -> u64 {
        self.values
            .iter()
            .map(|v| v.load(Ordering::Relaxed))
            .sum()
    }
}

impl Default for PerCpuAtomicCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// A per-CPU flag using atomics.
pub struct PerCpuBool {
    values: [AtomicBool; MAX_CPUS],
}

impl PerCpuBool {
    /// Create a new per-CPU bool initialized to false.
    pub fn new() -> Self {
        Self {
            values: core::array::from_fn(|_| AtomicBool::new(false)),
        }
    }

    /// Set the flag for a specific CPU.
    pub fn set(&self, cpu_id: usize, val: bool) {
        debug_assert!(cpu_id < MAX_CPUS);
        self.values[cpu_id].store(val, Ordering::Relaxed);
    }

    /// Read the flag for a specific CPU.
    pub fn get(&self, cpu_id: usize) -> bool {
        debug_assert!(cpu_id < MAX_CPUS);
        self.values[cpu_id].load(Ordering::Relaxed)
    }

    /// Returns true if any CPU has the flag set.
    pub fn any(&self) -> bool {
        self.values.iter().any(|v| v.load(Ordering::Relaxed))
    }

    /// Returns true if all CPUs have the flag set.
    pub fn all(&self) -> bool {
        self.values.iter().all(|v| v.load(Ordering::Relaxed))
    }
}

impl Default for PerCpuBool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial;

    #[test]
    fn percpu_counter_inc() {
        let _s = test_serial::acquire();
        let counter = PerCpuCounter::new();
        counter.inc(0);
        counter.inc(0);
        counter.inc(1);
        assert_eq!(counter.get(0), 2);
        assert_eq!(counter.get(1), 1);
        assert_eq!(counter.sum_all(), 3);
    }

    #[test]
    fn percpu_counter_add() {
        let _s = test_serial::acquire();
        let counter = PerCpuCounter::new();
        counter.add(0, 10);
        counter.add(0, 20);
        assert_eq!(counter.get(0), 30);
    }

    #[test]
    fn percpu_counter_reset() {
        let _s = test_serial::acquire();
        let counter = PerCpuCounter::new();
        counter.add(0, 5);
        counter.add(1, 10);
        counter.reset_all();
        assert_eq!(counter.sum_all(), 0);
    }

    #[test]
    fn percpu_atomic_counter() {
        let _s = test_serial::acquire();
        let counter = PerCpuAtomicCounter::new();
        counter.inc(0);
        counter.add(1, 42);
        assert_eq!(counter.get(0), 1);
        assert_eq!(counter.get(1), 42);
        assert_eq!(counter.sum_all(), 43);
    }

    #[test]
    fn percpu_bool() {
        let _s = test_serial::acquire();
        let flags = PerCpuBool::new();
        assert!(!flags.any());

        flags.set(3, true);
        assert!(flags.get(3));
        assert!(flags.any());
    }
}
