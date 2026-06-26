//! Runtime power management with reference-counted autosuspend.
//!
//! Provides `runtime_get_sync()` / `runtime_put_suspend()` API for
//! device drivers to manage runtime power states. Devices are auto-suspended
//! after a configurable idle timeout when the reference count drops to zero.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

/// Runtime PM state for a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePmState {
    /// Device is active (fully powered).
    Active,
    /// Device is runtime-suspended.
    Suspended,
    /// Device is in the process of suspending.
    Suspending,
    /// Device is in the process of resuming.
    Resuming,
}

/// Per-device runtime PM data.
#[derive(Debug)]
struct RuntimePmDevice {
    device_id: u32,
    state: RuntimePmState,
    /// Reference count (positive = active users, 0 = idle).
    ref_count: i32,
    /// Number of times the device was runtime suspended.
    suspend_count: u64,
    /// Number of times the device was runtime resumed.
    resume_count: u64,
    /// Auto-suspend delay in ticks (0 = suspend immediately when idle).
    auto_suspend_delay: u64,
    /// Timestamp of last put (for auto-suspend calculation).
    last_put_ticks: u64,
    /// Whether runtime PM is enabled for this device.
    enabled: bool,
}

/// Callback type for runtime suspend/resume.
pub type RuntimeSuspendCallback = fn(device_id: u32) -> bool;
pub type RuntimeResumeCallback = fn(device_id: u32) -> bool;

struct RuntimePmCallbacks {
    suspend: Option<RuntimeSuspendCallback>,
    resume: Option<RuntimeResumeCallback>,
}

/// Global runtime PM state.
struct RuntimePmManager {
    devices: Vec<RuntimePmDevice>,
    callbacks: Vec<RuntimePmCallbacks>,
    /// Global tick counter (starts at 1, incremented by timer).
    ticks: u64,
    /// Default auto-suspend delay for new devices.
    default_delay: u64,
}

static PM_MANAGER: Mutex<Option<RuntimePmManager>> = Mutex::new(None);

/// Whether auto-suspend is globally enabled.
static AUTO_SUSPEND_ENABLED: AtomicBool = AtomicBool::new(true);

/// Initialize runtime PM.
pub fn init_runtime_pm(default_auto_suspend_delay: u64) {
    *PM_MANAGER.lock() = Some(RuntimePmManager {
        devices: Vec::new(),
        callbacks: Vec::new(),
        ticks: 1,
        default_delay: default_auto_suspend_delay,
    });
    AUTO_SUSPEND_ENABLED.store(true, Ordering::Relaxed);
}

/// Reset runtime PM (for tests).
pub fn reset_runtime_pm() {
    *PM_MANAGER.lock() = None;
    AUTO_SUSPEND_ENABLED.store(true, Ordering::Relaxed);
}

/// Register a device for runtime PM. Returns the runtime PM index.
pub fn runtime_pm_register(
    device_id: u32,
    suspend: Option<RuntimeSuspendCallback>,
    resume: Option<RuntimeResumeCallback>,
) -> bool {
    let mut mgr = PM_MANAGER.lock();
    let m = mgr.as_mut().expect("runtime PM not initialized");

    if m.devices.iter().any(|d| d.device_id == device_id) {
        return false; // Already registered
    }

    m.devices.push(RuntimePmDevice {
        device_id,
        state: RuntimePmState::Active,
        ref_count: 0,
        suspend_count: 0,
        resume_count: 0,
        auto_suspend_delay: m.default_delay,
        last_put_ticks: 0,
        enabled: true,
    });

    m.callbacks.push(RuntimePmCallbacks { suspend, resume });
    true
}

/// Unregister a device from runtime PM.
pub fn runtime_pm_unregister(device_id: u32) -> bool {
    let mut mgr = PM_MANAGER.lock();
    let m = mgr.as_mut().expect("runtime PM not initialized");
    if let Some(idx) = m.devices.iter().position(|d| d.device_id == device_id) {
        m.devices.remove(idx);
        m.callbacks.remove(idx);
        true
    } else {
        false
    }
}

/// Get a reference to the device (increment ref count, resume if needed).
/// Returns the new ref count, or -1 on error.
pub fn runtime_get_sync(device_id: u32) -> i32 {
    let mut mgr = PM_MANAGER.lock();
    let m = mgr.as_mut().expect("runtime PM not initialized");

    let idx = match m.devices.iter().position(|d| d.device_id == device_id) {
        Some(i) => i,
        None => return -1,
    };

    if !m.devices[idx].enabled {
        return -1;
    }

    m.devices[idx].ref_count += 1;

    // If suspended, resume
    if m.devices[idx].state == RuntimePmState::Suspended {
        m.devices[idx].state = RuntimePmState::Resuming;
        let result = if let Some(resume_fn) = m.callbacks[idx].resume {
            resume_fn(device_id)
        } else {
            true
        };

        if result {
            m.devices[idx].state = RuntimePmState::Active;
            m.devices[idx].resume_count += 1;
        } else {
            m.devices[idx].state = RuntimePmState::Suspended;
            m.devices[idx].ref_count -= 1;
            return -1;
        }
    }

    m.devices[idx].ref_count
}

/// Drop a reference to the device (decrement ref count, auto-suspend if idle).
/// Returns the new ref count, or -1 on error.
pub fn runtime_put_suspend(device_id: u32) -> i32 {
    let mut mgr = PM_MANAGER.lock();
    let m = mgr.as_mut().expect("runtime PM not initialized");

    let idx = match m.devices.iter().position(|d| d.device_id == device_id) {
        Some(i) => i,
        None => return -1,
    };

    if m.devices[idx].ref_count <= 0 {
        return -1;
    }

    m.devices[idx].ref_count -= 1;
    m.devices[idx].last_put_ticks = m.ticks;

    // If ref count reaches 0 and auto-suspend is enabled, suspend immediately
    // (for testing; production would use the delay)
    if m.devices[idx].ref_count == 0
        && AUTO_SUSPEND_ENABLED.load(Ordering::Relaxed)
        && m.devices[idx].auto_suspend_delay == 0
    {
        let dev_id = m.devices[idx].device_id;
        m.devices[idx].state = RuntimePmState::Suspending;

        let result = if let Some(suspend_fn) = m.callbacks[idx].suspend {
            suspend_fn(dev_id)
        } else {
            true
        };

        if result {
            m.devices[idx].state = RuntimePmState::Suspended;
            m.devices[idx].suspend_count += 1;
        } else {
            m.devices[idx].state = RuntimePmState::Active;
        }
    }

    m.devices[idx].ref_count
}

/// Check all devices for auto-suspend eligibility. Called periodically.
/// Returns the number of devices auto-suspended.
pub fn runtime_pm_tick() -> u32 {
    let mut mgr = PM_MANAGER.lock();
    let m = match mgr.as_mut() {
        Some(m) => m,
        None => return 0,
    };

    m.ticks += 1;
    let current_tick = m.ticks;
    let mut suspended = 0;

    if !AUTO_SUSPEND_ENABLED.load(Ordering::Relaxed) {
        return 0;
    }

    let device_count = m.devices.len();
    for i in 0..device_count {
        if m.devices[i].ref_count > 0
            || m.devices[i].state != RuntimePmState::Active
            || !m.devices[i].enabled
            || m.devices[i].auto_suspend_delay == 0
        {
            continue;
        }

        let elapsed = current_tick.saturating_sub(m.devices[i].last_put_ticks);
        if elapsed >= m.devices[i].auto_suspend_delay && m.devices[i].last_put_ticks > 0 {
            let dev_id = m.devices[i].device_id;
            m.devices[i].state = RuntimePmState::Suspending;

            let result = if let Some(suspend_fn) = m.callbacks[i].suspend {
                suspend_fn(dev_id)
            } else {
                true
            };

            if result {
                m.devices[i].state = RuntimePmState::Suspended;
                m.devices[i].suspend_count += 1;
                suspended += 1;
            } else {
                m.devices[i].state = RuntimePmState::Active;
            }
        }
    }

    suspended
}

/// Set the auto-suspend delay for a device.
pub fn runtime_pm_set_delay(device_id: u32, delay_ticks: u64) -> bool {
    let mut mgr = PM_MANAGER.lock();
    let m = mgr.as_mut().expect("runtime PM not initialized");
    if let Some(dev) = m.devices.iter_mut().find(|d| d.device_id == device_id) {
        dev.auto_suspend_delay = delay_ticks;
        true
    } else {
        false
    }
}

/// Enable or disable runtime PM for a device.
pub fn runtime_pm_set_enabled(device_id: u32, enabled: bool) -> bool {
    let mut mgr = PM_MANAGER.lock();
    let m = mgr.as_mut().expect("runtime PM not initialized");
    if let Some(dev) = m.devices.iter_mut().find(|d| d.device_id == device_id) {
        dev.enabled = enabled;
        true
    } else {
        false
    }
}

/// Get runtime PM state for a device.
pub fn runtime_pm_get_state(device_id: u32) -> Option<(RuntimePmState, i32, u64, u64)> {
    let mgr = PM_MANAGER.lock();
    let m = mgr.as_ref()?;
    m.devices.iter().find(|d| d.device_id == device_id).map(|d| {
        (d.state, d.ref_count, d.suspend_count, d.resume_count)
    })
}

/// Enable or disable auto-suspend globally.
pub fn set_auto_suspend_enabled(enabled: bool) {
    AUTO_SUSPEND_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Get the current tick count.
pub fn runtime_pm_ticks() -> u64 {
    PM_MANAGER.lock().as_ref().map(|m| m.ticks).unwrap_or(0)
}

/// Get total runtime-suspended device count.
pub fn runtime_pm_suspended_count() -> usize {
    PM_MANAGER.lock().as_ref()
        .map(|m| m.devices.iter().filter(|d| d.state == RuntimePmState::Suspended).count())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rsuspend(_device_id: u32) -> bool { true }
    fn test_rresume(_device_id: u32) -> bool { true }
    fn failing_rsuspend(_device_id: u32) -> bool { false }

    #[test]
    fn register_and_get() {
        let _s = crate::test_serial::acquire();
        reset_runtime_pm();
        init_runtime_pm(0);
        assert!(runtime_pm_register(1, Some(test_rsuspend), Some(test_rresume)));
        let refs = runtime_get_sync(1);
        assert_eq!(refs, 1);
    }

    #[test]
    fn get_put_round_trip() {
        let _s = crate::test_serial::acquire();
        reset_runtime_pm();
        init_runtime_pm(0);
        runtime_pm_register(1, Some(test_rsuspend), Some(test_rresume));

        runtime_get_sync(1);
        runtime_get_sync(1);
        let refs = runtime_put_suspend(1);
        assert_eq!(refs, 1);
        let refs = runtime_put_suspend(1);
        assert_eq!(refs, 0);
    }

    #[test]
    fn auto_suspend_on_zero_refs() {
        let _s = crate::test_serial::acquire();
        reset_runtime_pm();
        init_runtime_pm(0); // delay=0 means immediate
        set_auto_suspend_enabled(true);
        runtime_pm_register(1, Some(test_rsuspend), Some(test_rresume));

        runtime_get_sync(1);
        runtime_put_suspend(1);

        let (state, _, suspend_count, _) = runtime_pm_get_state(1).unwrap();
        assert_eq!(state, RuntimePmState::Suspended);
        assert_eq!(suspend_count, 1);
    }

    #[test]
    fn get_resumes_suspended_device() {
        let _s = crate::test_serial::acquire();
        reset_runtime_pm();
        init_runtime_pm(0);
        set_auto_suspend_enabled(true);
        runtime_pm_register(1, Some(test_rsuspend), Some(test_rresume));

        runtime_get_sync(1);
        runtime_put_suspend(1); // auto-suspend
        let (state, _, _, _) = runtime_pm_get_state(1).unwrap();
        assert_eq!(state, RuntimePmState::Suspended);

        runtime_get_sync(1); // should resume
        let (state, refs, _, resume_count) = runtime_pm_get_state(1).unwrap();
        assert_eq!(state, RuntimePmState::Active);
        assert_eq!(refs, 1);
        assert_eq!(resume_count, 1);
    }

    #[test]
    fn unregister() {
        let _s = crate::test_serial::acquire();
        reset_runtime_pm();
        init_runtime_pm(0);
        runtime_pm_register(1, None, None);
        assert!(runtime_pm_unregister(1));
        assert!(!runtime_pm_unregister(1));
    }

    #[test]
    fn set_delay() {
        let _s = crate::test_serial::acquire();
        reset_runtime_pm();
        init_runtime_pm(0);
        runtime_pm_register(1, None, None);
        assert!(runtime_pm_set_delay(1, 100));
        assert!(!runtime_pm_set_delay(999, 100));
    }

    #[test]
    fn tick_triggers_auto_suspend() {
        let _s = crate::test_serial::acquire();
        reset_runtime_pm();
        init_runtime_pm(5); // 5 tick delay
        set_auto_suspend_enabled(true);
        runtime_pm_register(1, Some(test_rsuspend), Some(test_rresume));

        runtime_get_sync(1);
        runtime_put_suspend(1);

        // Not enough ticks yet
        for _ in 0..4 {
            runtime_pm_tick();
        }
        let (state, _, _, _) = runtime_pm_get_state(1).unwrap();
        assert_eq!(state, RuntimePmState::Active);

        // 5th tick should trigger
        runtime_pm_tick();
        let (state, _, _, _) = runtime_pm_get_state(1).unwrap();
        assert_eq!(state, RuntimePmState::Suspended);
    }

    #[test]
    fn disable_global_auto_suspend() {
        let _s = crate::test_serial::acquire();
        reset_runtime_pm();
        init_runtime_pm(0);
        set_auto_suspend_enabled(false);
        runtime_pm_register(1, Some(test_rsuspend), Some(test_rresume));

        runtime_get_sync(1);
        runtime_put_suspend(1);

        let (state, _, _, _) = runtime_pm_get_state(1).unwrap();
        assert_eq!(state, RuntimePmState::Active); // not suspended
    }

    #[test]
    fn disable_per_device() {
        let _s = crate::test_serial::acquire();
        reset_runtime_pm();
        init_runtime_pm(0);
        set_auto_suspend_enabled(true);
        runtime_pm_register(1, Some(test_rsuspend), Some(test_rresume));
        runtime_pm_set_enabled(1, false);

        runtime_get_sync(1);
        let refs = runtime_put_suspend(1);
        assert_eq!(refs, -1); // disabled
    }

    #[test]
    fn failing_suspend_keeps_active() {
        let _s = crate::test_serial::acquire();
        reset_runtime_pm();
        init_runtime_pm(0);
        set_auto_suspend_enabled(true);
        runtime_pm_register(1, Some(failing_rsuspend), Some(test_rresume));

        runtime_get_sync(1);
        runtime_put_suspend(1);

        let (state, _, _, _) = runtime_pm_get_state(1).unwrap();
        assert_eq!(state, RuntimePmState::Active); // suspend failed
    }

    #[test]
    fn put_with_zero_refs_fails() {
        let _s = crate::test_serial::acquire();
        reset_runtime_pm();
        init_runtime_pm(0);
        runtime_pm_register(1, None, None);
        assert_eq!(runtime_put_suspend(1), -1);
    }

    #[test]
    fn get_nonexistent_fails() {
        let _s = crate::test_serial::acquire();
        reset_runtime_pm();
        init_runtime_pm(0);
        assert_eq!(runtime_get_sync(999), -1);
    }

    #[test]
    fn multiple_devices_independent() {
        let _s = crate::test_serial::acquire();
        reset_runtime_pm();
        init_runtime_pm(0);
        set_auto_suspend_enabled(true);
        runtime_pm_register(1, Some(test_rsuspend), Some(test_rresume));
        runtime_pm_register(2, Some(test_rsuspend), Some(test_rresume));

        runtime_get_sync(1);
        runtime_put_suspend(1); // device 1 suspends

        let (state1, _, _, _) = runtime_pm_get_state(1).unwrap();
        let (state2, _, _, _) = runtime_pm_get_state(2).unwrap();
        assert_eq!(state1, RuntimePmState::Suspended);
        assert_eq!(state2, RuntimePmState::Active);
    }

    #[test]
    fn suspended_count() {
        let _s = crate::test_serial::acquire();
        reset_runtime_pm();
        init_runtime_pm(0);
        set_auto_suspend_enabled(true);
        runtime_pm_register(1, Some(test_rsuspend), Some(test_rresume));
        runtime_pm_register(2, Some(test_rsuspend), Some(test_rresume));

        runtime_get_sync(1);
        runtime_put_suspend(1);
        runtime_get_sync(2);
        runtime_put_suspend(2);

        assert_eq!(runtime_pm_suspended_count(), 2);
    }
}
