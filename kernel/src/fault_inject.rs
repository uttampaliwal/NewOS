//! Fault injection framework for robustness testing.
//!
//! Provides configurable failure points for allocation, I/O, and network
//! operations. Each fault point has a name, a probability, and a counter.
//! When checked, it randomly decides whether to inject a failure based on
//! the configured probability.

use alloc::vec::Vec;
use spin::Mutex;

/// Maximum number of registered fault points.
const MAX_FAULT_POINTS: usize = 64;

/// The type of subsystem a fault point belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultType {
    /// Memory allocation failure.
    Alloc,
    /// Block I/O failure.
    Io,
    /// Network operation failure.
    Network,
    /// Filesystem operation failure.
    FileSystem,
    /// Generic / other failure.
    Other,
}

/// A registered fault injection point.
#[derive(Debug, Clone)]
pub struct FaultPoint {
    pub name: &'static str,
    pub fault_type: FaultType,
    /// Probability of failure on each check (0.0 = never, 1.0 = always).
    pub probability: f64,
    /// Number of times this fault point has been checked.
    pub check_count: u64,
    /// Number of times a fault was actually injected.
    pub inject_count: u64,
    /// Whether this fault point is enabled.
    pub enabled: bool,
    /// If > 0, inject failure every N-th check (overrides probability).
    pub every_n: u64,
    /// Counter for every-n mode.
    pub n_counter: u64,
}

/// Global fault injection state.
struct FaultInjector {
    points: Vec<FaultPoint>,
    /// Total faults injected across all points.
    total_injections: u64,
    /// Global enable/disable switch.
    enabled: bool,
    /// Simple PRNG state (xorshift64).
    rng_state: u64,
}

static INJECTOR: Mutex<FaultInjector> = Mutex::new(FaultInjector {
    points: Vec::new(),
    total_injections: 0,
    enabled: true,
    rng_state: 0x1234_5678_9ABC_DEF0,
});

/// Initialize the fault injection framework.
pub fn init_fault_inject() {
    let mut inj = INJECTOR.lock();
    inj.points.clear();
    inj.total_injections = 0;
    inj.enabled = true;
    inj.rng_state = 0x1234_5678_9ABC_DEF0;
}

/// Reset the fault injection framework (for tests).
pub fn reset_fault_inject() {
    *INJECTOR.lock() = FaultInjector {
        points: Vec::new(),
        total_injections: 0,
        enabled: true,
        rng_state: 0x1234_5678_9ABC_DEF0,
    };
}

/// Register a fault point. Returns true if registered successfully.
pub fn register_fault_point(
    name: &'static str,
    fault_type: FaultType,
    probability: f64,
) -> bool {
    let mut inj = INJECTOR.lock();
    if inj.points.len() >= MAX_FAULT_POINTS {
        return false;
    }
    if inj.points.iter().any(|p| p.name == name) {
        return false;
    }
    inj.points.push(FaultPoint {
        name,
        fault_type,
        probability: probability.clamp(0.0, 1.0),
        check_count: 0,
        inject_count: 0,
        enabled: true,
        every_n: 0,
        n_counter: 0,
    });
    true
}

/// Register a fault point that fires every N-th check.
pub fn register_fault_point_every_n(
    name: &'static str,
    fault_type: FaultType,
    n: u64,
) -> bool {
    let mut inj = INJECTOR.lock();
    if inj.points.len() >= MAX_FAULT_POINTS || n == 0 {
        return false;
    }
    if inj.points.iter().any(|p| p.name == name) {
        return false;
    }
    inj.points.push(FaultPoint {
        name,
        fault_type,
        probability: 0.0,
        check_count: 0,
        inject_count: 0,
        enabled: true,
        every_n: n,
        n_counter: 0,
    });
    true
}

/// Enable or disable a specific fault point by name.
pub fn set_fault_enabled(name: &str, enabled: bool) -> bool {
    let mut inj = INJECTOR.lock();
    if let Some(point) = inj.points.iter_mut().find(|p| p.name == name) {
        point.enabled = enabled;
        true
    } else {
        false
    }
}

/// Update the probability of a fault point.
pub fn set_fault_probability(name: &str, probability: f64) -> bool {
    let mut inj = INJECTOR.lock();
    if let Some(point) = inj.points.iter_mut().find(|p| p.name == name) {
        point.probability = probability.clamp(0.0, 1.0);
        true
    } else {
        false
    }
}

/// Enable or disable the global fault injection switch.
pub fn set_global_fault_enabled(enabled: bool) {
    INJECTOR.lock().enabled = enabled;
}

/// Check a fault point. Returns `true` if a fault should be injected.
pub fn should_fault(name: &str) -> bool {
    let mut inj = INJECTOR.lock();
    if !inj.enabled {
        return false;
    }

    let point_idx = match inj.points.iter().position(|p| p.name == name) {
        Some(i) => i,
        None => return false,
    };

    if !inj.points[point_idx].enabled {
        return false;
    }

    inj.points[point_idx].check_count += 1;

    // Check every-n mode first
    if inj.points[point_idx].every_n > 0 {
        inj.points[point_idx].n_counter += 1;
        if inj.points[point_idx].n_counter >= inj.points[point_idx].every_n {
            inj.points[point_idx].n_counter = 0;
            inj.points[point_idx].inject_count += 1;
            inj.total_injections += 1;
            return true;
        }
        return false;
    }

    // Probability-based injection
    let probability = inj.points[point_idx].probability;
    if probability > 0.0 {
        let rand_val = next_random(&mut inj.rng_state);
        let threshold = (probability * 1_000_000.0) as u64;
        if (rand_val % 1_000_000) < threshold {
            inj.points[point_idx].inject_count += 1;
            inj.total_injections += 1;
            return true;
        }
    }

    false
}

/// Force-inject a fault at a specific point (for testing).
pub fn force_fault(name: &str) -> bool {
    let mut inj = INJECTOR.lock();
    if let Some(point) = inj.points.iter_mut().find(|p| p.name == name) {
        point.check_count += 1;
        point.inject_count += 1;
        inj.total_injections += 1;
        true
    } else {
        false
    }
}

/// Get the list of registered fault points.
pub fn list_fault_points() -> Vec<FaultPoint> {
    INJECTOR.lock().points.clone()
}

/// Get total injections across all fault points.
pub fn total_injections() -> u64 {
    INJECTOR.lock().total_injections
}

/// Get the injection count for a specific fault point.
pub fn fault_injection_count(name: &str) -> u64 {
    INJECTOR
        .lock()
        .points
        .iter()
        .find(|p| p.name == name)
        .map(|p| p.inject_count)
        .unwrap_or(0)
}

/// Get the check count for a specific fault point.
pub fn fault_check_count(name: &str) -> u64 {
    INJECTOR
        .lock()
        .points
        .iter()
        .find(|p| p.name == name)
        .map(|p| p.check_count)
        .unwrap_or(0)
}

/// Unregister a fault point.
pub fn unregister_fault_point(name: &str) -> bool {
    let mut inj = INJECTOR.lock();
    let len_before = inj.points.len();
    inj.points.retain(|p| p.name != name);
    inj.points.len() < len_before
}

/// Simple xorshift64 PRNG.
fn next_random(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_check() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        assert!(register_fault_point("test_alloc", FaultType::Alloc, 0.0));
        assert!(should_fault("test_alloc") == false); // 0% probability
        assert_eq!(fault_check_count("test_alloc"), 1);
    }

    #[test]
    fn always_fault() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        register_fault_point("always", FaultType::Io, 1.0);
        for _ in 0..100 {
            assert!(should_fault("always"));
        }
        assert_eq!(fault_injection_count("always"), 100);
    }

    #[test]
    fn never_fault() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        register_fault_point("never", FaultType::Network, 0.0);
        for _ in 0..1000 {
            assert!(!should_fault("never"));
        }
        assert_eq!(fault_injection_count("never"), 0);
    }

    #[test]
    fn every_n_fault() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        register_fault_point_every_n("every3", FaultType::Alloc, 3);

        for i in 0..9 {
            let result = should_fault("every3");
            if (i + 1) % 3 == 0 {
                assert!(result, "should fault at check {}", i + 1);
            } else {
                assert!(!result, "should NOT fault at check {}", i + 1);
            }
        }
        assert_eq!(fault_injection_count("every3"), 3);
        assert_eq!(fault_check_count("every3"), 9);
    }

    #[test]
    fn enable_disable_fault() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        register_fault_point("toggle", FaultType::Io, 1.0);
        assert!(should_fault("toggle"));

        set_fault_enabled("toggle", false);
        assert!(!should_fault("toggle"));

        set_fault_enabled("toggle", true);
        assert!(should_fault("toggle"));
    }

    #[test]
    fn global_disable() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        register_fault_point("global", FaultType::Alloc, 1.0);
        assert!(should_fault("global"));

        set_global_fault_enabled(false);
        assert!(!should_fault("global"));

        set_global_fault_enabled(true);
        assert!(should_fault("global"));
    }

    #[test]
    fn duplicate_name_rejected() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        assert!(register_fault_point("dup", FaultType::Alloc, 0.5));
        assert!(!register_fault_point("dup", FaultType::Io, 0.5));
    }

    #[test]
    fn unregister() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        register_fault_point("removable", FaultType::Alloc, 0.5);
        assert!(unregister_fault_point("removable"));
        assert!(!unregister_fault_point("removable"));
        assert!(!should_fault("removable"));
    }

    #[test]
    fn list_points() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        register_fault_point("a", FaultType::Alloc, 0.1);
        register_fault_point("b", FaultType::Io, 0.2);
        let list = list_fault_points();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn force_fault_works() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        register_fault_point("forced", FaultType::Alloc, 0.0);
        assert!(force_fault("forced"));
        assert_eq!(fault_injection_count("forced"), 1);
    }

    #[test]
    fn force_nonexistent_returns_false() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        assert!(!force_fault("nope"));
    }

    #[test]
    fn total_injections_counter() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        register_fault_point("x", FaultType::Io, 1.0);
        register_fault_point("y", FaultType::Io, 1.0);
        should_fault("x");
        should_fault("y");
        should_fault("x");
        assert_eq!(total_injections(), 3);
    }

    #[test]
    fn probability_half() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        register_fault_point("half", FaultType::Alloc, 0.5);
        let mut faults = 0u64;
        for _ in 0..10000 {
            if should_fault("half") {
                faults += 1;
            }
        }
        // With 10000 checks at 50%, expect ~5000 (allow wide margin)
        assert!(faults > 4000 && faults < 6000, "got {}", faults);
    }

    #[test]
    fn update_probability() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        register_fault_point("upd", FaultType::Alloc, 0.0);
        assert!(!should_fault("upd"));

        set_fault_probability("upd", 1.0);
        assert!(should_fault("upd"));
    }

    #[test]
    fn nonexistent_point_returns_false() {
        let _s = crate::test_serial::acquire();
        reset_fault_inject();
        assert!(!should_fault("ghost"));
    }
}
