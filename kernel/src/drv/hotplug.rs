//! Hotplug framework — device insertion/removal event system.
//!
//! Provides an event-driven mechanism for dynamic device addition and removal.
//! Listeners can register callbacks for hotplug events, and the framework
//! coordinates probe/removal through the driver model.

use alloc::{string::String, vec::Vec};
use spin::Mutex;

use super::{BusType, probe_device, remove_driver, register_device, unregister_device};

/// Types of hotplug events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotplugEventType {
    /// A device was inserted/attached.
    DeviceAdd,
    /// A device was removed/detached.
    DeviceRemove,
    /// A device was bound to a driver.
    DriverBind,
    /// A device was unbound from a driver.
    DriverUnbind,
}

/// A hotplug event.
#[derive(Debug, Clone)]
pub struct HotplugEvent {
    /// Type of event.
    pub event_type: HotplugEventType,
    /// Device ID associated with the event.
    pub device_id: u32,
    /// Bus type of the device.
    pub bus_type: BusType,
    /// Timestamp (tick count when event occurred).
    pub timestamp: u64,
}

/// Callback type for hotplug event listeners.
pub type HotplugListenerCallback = fn(&HotplugEvent) -> bool;

/// A registered hotplug event listener.
struct HotplugListener {
    id: u32,
    name: String,
    event_mask: u32, // bitmask of HotplugEventType
    callback: HotplugListenerCallback,
    enabled: bool,
}

/// Global hotplug state.
struct HotplugManager {
    listeners: Vec<HotplugListener>,
    event_log: Vec<HotplugEvent>,
    next_listener_id: u32,
    tick: u64,
    max_log_size: usize,
    /// Pending add events (device_id, bus_type).
    pending_adds: Vec<(u32, BusType)>,
    /// Pending remove events (device_id).
    pending_removes: Vec<u32>,
}

static HOTPLUG: Mutex<Option<HotplugManager>> = Mutex::new(None);

const fn event_to_bit(event: HotplugEventType) -> u32 {
    match event {
        HotplugEventType::DeviceAdd => 1 << 0,
        HotplugEventType::DeviceRemove => 1 << 1,
        HotplugEventType::DriverBind => 1 << 2,
        HotplugEventType::DriverUnbind => 1 << 3,
    }
}

/// All events mask.
const ALL_EVENTS: u32 = 0xF;

/// Initialize the hotplug framework.
pub fn init_hotplug(max_log_size: usize) {
    *HOTPLUG.lock() = Some(HotplugManager {
        listeners: Vec::new(),
        event_log: Vec::new(),
        next_listener_id: 1,
        tick: 0,
        max_log_size,
        pending_adds: Vec::new(),
        pending_removes: Vec::new(),
    });
}

/// Reset hotplug framework (for tests).
pub fn reset_hotplug() {
    *HOTPLUG.lock() = None;
}

/// Register a hotplug event listener. Returns the listener ID.
pub fn hotplug_register_listener(
    name: &str,
    event_mask: u32,
    callback: HotplugListenerCallback,
) -> u32 {
    let mut mgr = HOTPLUG.lock();
    let m = mgr.as_mut().expect("hotplug not initialized");
    let id = m.next_listener_id;
    m.next_listener_id += 1;

    m.listeners.push(HotplugListener {
        id,
        name: String::from(name),
        event_mask,
        callback,
        enabled: true,
    });

    id
}

/// Unregister a hotplug listener.
pub fn hotplug_unregister_listener(listener_id: u32) -> bool {
    let mut mgr = HOTPLUG.lock();
    let m = mgr.as_mut().expect("hotplug not initialized");
    let len_before = m.listeners.len();
    m.listeners.retain(|l| l.id != listener_id);
    m.listeners.len() < len_before
}

/// Enable or disable a listener.
pub fn hotplug_set_listener_enabled(listener_id: u32, enabled: bool) -> bool {
    let mut mgr = HOTPLUG.lock();
    let m = mgr.as_mut().expect("hotplug not initialized");
    if let Some(l) = m.listeners.iter_mut().find(|l| l.id == listener_id) {
        l.enabled = enabled;
        true
    } else {
        false
    }
}

/// Emit a hotplug event, notifying all matching listeners.
fn emit_event(event: &HotplugEvent) {
    let mgr = HOTPLUG.lock();
    let m = match mgr.as_ref() {
        Some(m) => m,
        None => return,
    };

    let bit = event_to_bit(event.event_type);

    for listener in m.listeners.iter() {
        if listener.enabled && (listener.event_mask & bit) != 0 {
            let _ = (listener.callback)(event);
        }
    }
}

/// Add an event to the log.
fn log_event(m: &mut HotplugManager, event: HotplugEvent) {
    if m.event_log.len() >= m.max_log_size {
        m.event_log.remove(0);
    }
    m.event_log.push(event);
}

/// Queue a device addition (processed by `hotplug_process_pending`).
pub fn hotplug_add_device(name: &str, bus_type: BusType, capabilities: u32) -> u32 {
    let device_id = register_device(name, bus_type, capabilities);

    let mut mgr = HOTPLUG.lock();
    let m = mgr.as_mut().expect("hotplug not initialized");
    m.pending_adds.push((device_id, bus_type));

    device_id
}

/// Queue a device removal (processed by `hotplug_process_pending`).
pub fn hotplug_remove_device(device_id: u32) -> bool {
    let mut mgr = HOTPLUG.lock();
    let m = mgr.as_mut().expect("hotplug not initialized");
    if m.pending_removes.contains(&device_id) {
        return false;
    }
    m.pending_removes.push(device_id);
    true
}

/// Process all pending add and remove events. Returns (added, removed) counts.
pub fn hotplug_process_pending() -> (u32, u32) {
    // Phase 1: snapshot pending events while holding the lock
    let (tick, pending_adds, pending_removes) = {
        let mut mgr = HOTPLUG.lock();
        let m = mgr.as_mut().expect("hotplug not initialized");
        m.tick += 1;
        let tick = m.tick;
        let pending_adds: Vec<(u32, BusType)> = m.pending_adds.drain(..).collect();
        let pending_removes: Vec<u32> = m.pending_removes.drain(..).collect();
        (tick, pending_adds, pending_removes)
    };

    let mut added = 0u32;
    let mut removed = 0u32;

    // Phase 2: process adds (no locks held)
    for (device_id, bus_type) in pending_adds {
        let event = HotplugEvent {
            event_type: HotplugEventType::DeviceAdd,
            device_id,
            bus_type,
            timestamp: tick,
        };

        // Log event
        {
            let mut mgr = HOTPLUG.lock();
            if let Some(m) = mgr.as_mut() {
                log_event(m, event.clone());
            }
        }

        emit_event(&event);

        // Attempt probe
        let probed = probe_device(device_id);

        if probed {
            let bind_event = HotplugEvent {
                event_type: HotplugEventType::DriverBind,
                device_id,
                bus_type,
                timestamp: tick,
            };
            {
                let mut mgr = HOTPLUG.lock();
                if let Some(m) = mgr.as_mut() {
                    log_event(m, bind_event.clone());
                }
            }
            emit_event(&bind_event);
        }

        added += 1;
    }

    // Phase 3: process removes (no locks held)
    for device_id in pending_removes {
        remove_driver(device_id);

        let event = HotplugEvent {
            event_type: HotplugEventType::DeviceRemove,
            device_id,
            bus_type: BusType::Pci,
            timestamp: tick,
        };

        {
            let mut mgr = HOTPLUG.lock();
            if let Some(m) = mgr.as_mut() {
                log_event(m, event.clone());
            }
        }

        emit_event(&event);
        unregister_device(device_id);

        removed += 1;
    }

    (added, removed)
}

/// Get the event log.
pub fn hotplug_get_log() -> Vec<HotplugEvent> {
    let mgr = HOTPLUG.lock();
    let m = match mgr.as_ref() {
        Some(m) => m,
        None => return Vec::new(),
    };
    m.event_log.clone()
}

/// Get the event log length.
pub fn hotplug_log_len() -> usize {
    HOTPLUG.lock().as_ref().map(|m| m.event_log.len()).unwrap_or(0)
}

/// Clear the event log.
pub fn hotplug_clear_log() {
    if let Some(m) = HOTPLUG.lock().as_mut() {
        m.event_log.clear();
    }
}

/// Get the number of registered listeners.
pub fn hotplug_listener_count() -> usize {
    HOTPLUG.lock().as_ref().map(|m| m.listeners.len()).unwrap_or(0)
}

/// Increment the tick counter (for event timestamps).
pub fn hotplug_tick() {
    if let Some(m) = HOTPLUG.lock().as_mut() {
        m.tick += 1;
    }
}

/// Get the current tick.
pub fn hotplug_current_tick() -> u64 {
    HOTPLUG.lock().as_ref().map(|m| m.tick).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering};

    static CALLBACK_COUNT: AtomicU32 = AtomicU32::new(0);

    fn test_listener(_event: &HotplugEvent) -> bool {
        CALLBACK_COUNT.fetch_add(1, Ordering::SeqCst);
        true
    }

    fn filter_listener(_event: &HotplugEvent) -> bool {
        CALLBACK_COUNT.fetch_add(1, Ordering::SeqCst);
        true
    }

    fn failing_listener(_event: &HotplugEvent) -> bool {
        CALLBACK_COUNT.fetch_add(1, Ordering::SeqCst);
        false
    }

    fn setup() {
        reset_hotplug();
        init_hotplug(100);
        super::super::reset_driver_model();
        super::super::init_driver_model();
        CALLBACK_COUNT.store(0, Ordering::SeqCst);
    }

    #[test]
    fn register_and_unregister_listener() {
        let _s = crate::test_serial::acquire();
        setup();
        let id = hotplug_register_listener("test", ALL_EVENTS, test_listener);
        assert_eq!(hotplug_listener_count(), 1);
        assert!(hotplug_unregister_listener(id));
        assert_eq!(hotplug_listener_count(), 0);
    }

    #[test]
    fn unregister_nonexistent() {
        let _s = crate::test_serial::acquire();
        setup();
        assert!(!hotplug_unregister_listener(999));
    }

    #[test]
    fn add_device_and_process() {
        let _s = crate::test_serial::acquire();
        setup();
        hotplug_add_device("nic0", BusType::Pci, 0);
        let (added, removed) = hotplug_process_pending();
        assert_eq!(added, 1);
        assert_eq!(removed, 0);
    }

    #[test]
    fn add_device_emits_event() {
        let _s = crate::test_serial::acquire();
        setup();
        hotplug_register_listener("test", ALL_EVENTS, test_listener);
        hotplug_add_device("nic0", BusType::Pci, 0);
        hotplug_process_pending();
        // At least one callback (DeviceAdd)
        assert!(CALLBACK_COUNT.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn remove_device() {
        let _s = crate::test_serial::acquire();
        setup();
        let id = hotplug_add_device("nic0", BusType::Pci, 0);
        hotplug_process_pending();
        assert!(hotplug_remove_device(id));
        hotplug_process_pending();
    }

    #[test]
    fn event_log() {
        let _s = crate::test_serial::acquire();
        setup();
        hotplug_add_device("nic0", BusType::Pci, 0);
        hotplug_process_pending();
        let log = hotplug_get_log();
        assert!(log.len() >= 1);
        assert_eq!(log[0].event_type, HotplugEventType::DeviceAdd);
    }

    #[test]
    fn clear_log() {
        let _s = crate::test_serial::acquire();
        setup();
        hotplug_add_device("nic0", BusType::Pci, 0);
        hotplug_process_pending();
        assert!(hotplug_log_len() > 0);
        hotplug_clear_log();
        assert_eq!(hotplug_log_len(), 0);
    }

    #[test]
    fn listener_filtering() {
        let _s = crate::test_serial::acquire();
        setup();
        // Only listen for DeviceRemove
        hotplug_register_listener("rm", 1 << 1, filter_listener);
        hotplug_add_device("nic0", BusType::Pci, 0);
        hotplug_process_pending();
        // Should not be called for DeviceAdd
        assert_eq!(CALLBACK_COUNT.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn disabled_listener() {
        let _s = crate::test_serial::acquire();
        setup();
        let id = hotplug_register_listener("test", ALL_EVENTS, test_listener);
        hotplug_set_listener_enabled(id, false);
        hotplug_add_device("nic0", BusType::Pci, 0);
        hotplug_process_pending();
        assert_eq!(CALLBACK_COUNT.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn multiple_listeners() {
        let _s = crate::test_serial::acquire();
        setup();
        hotplug_register_listener("a", ALL_EVENTS, test_listener);
        hotplug_register_listener("b", ALL_EVENTS, test_listener);
        hotplug_add_device("nic0", BusType::Pci, 0);
        hotplug_process_pending();
        assert!(CALLBACK_COUNT.load(Ordering::SeqCst) >= 2);
    }

    #[test]
    fn log_size_limit() {
        let _s = crate::test_serial::acquire();
        setup();
        // Fill the log
        for _i in 0..150 {
            hotplug_add_device("dev", BusType::Pci, 0);
            hotplug_process_pending();
        }
        // Log should be capped at 100
        assert!(hotplug_log_len() <= 100);
    }

    #[test]
    fn pending_add_device_id() {
        let _s = crate::test_serial::acquire();
        setup();
        let id = hotplug_add_device("nic0", BusType::Pci, 0);
        assert!(id > 0);
        // Device should exist
        assert!(super::super::get_device(id).is_some());
    }

    #[test]
    fn failing_listener_still_logged() {
        let _s = crate::test_serial::acquire();
        setup();
        hotplug_register_listener("fail", ALL_EVENTS, failing_listener);
        hotplug_add_device("nic0", BusType::Pci, 0);
        hotplug_process_pending();
        // Event still logged even if listener fails
        assert!(hotplug_log_len() >= 1);
    }

    #[test]
    fn multiple_devices_and_removes() {
        let _s = crate::test_serial::acquire();
        setup();
        let id1 = hotplug_add_device("nic0", BusType::Pci, 0);
        let id2 = hotplug_add_device("nic1", BusType::Pci, 0);
        hotplug_process_pending();
        assert!(hotplug_remove_device(id1));
        assert!(hotplug_remove_device(id2));
        hotplug_process_pending();
    }

    #[test]
    fn duplicate_remove_rejected() {
        let _s = crate::test_serial::acquire();
        setup();
        let id = hotplug_add_device("nic0", BusType::Pci, 0);
        hotplug_process_pending();
        assert!(hotplug_remove_device(id));
        assert!(!hotplug_remove_device(id));
    }
}
