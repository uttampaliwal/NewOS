//! Device driver subsystem — Bus, Device, Driver model with lifecycle
//! management, runtime power management, and hotplug event framework.

pub mod runtime_pm;
pub mod hotplug;

use alloc::{string::String, vec::Vec};
use spin::Mutex;

/// Power states for a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevicePowerState {
    /// Device is fully active.
    Active,
    /// Device is runtime-suspended (low power).
    Suspended,
    /// Device is system-suspended (off or lowest power).
    SystemSuspended,
    /// Device is in the process of suspending.
    Suspending,
    /// Device is in the process of resuming.
    Resuming,
}

/// Device capabilities (bitmask).
pub const DEV_CAP_DMA: u32 = 1 << 0;
pub const DEV_CAP_IRQ: u32 = 1 << 1;
pub const DEV_CAP_MMIO: u32 = 1 << 2;
pub const DEV_CAP_PORT_IO: u32 = 1 << 3;
pub const DEV_CAP_PERSISTENT: u32 = 1 << 4;

/// A device registered in the system.
#[derive(Debug)]
pub struct Device {
    /// Unique device ID.
    pub id: u32,
    /// Device name.
    pub name: String,
    /// Bus this device belongs to.
    pub bus_type: BusType,
    /// Current power state.
    pub power_state: DevicePowerState,
    /// Device capabilities.
    pub capabilities: u32,
    /// Reference count for runtime PM.
    pub ref_count: u32,
    /// Whether the device is enabled.
    pub enabled: bool,
    /// Driver bound to this device (index into driver registry).
    pub driver_id: Option<u32>,
    /// Device-specific private data pointer.
    pub private_data: u64,
}

/// Types of buses in the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusType {
    Pci,
    Usb,
    Virtio,
    Platform,
    Isa,
}

/// A device driver registered with a bus.
#[derive(Debug, Clone)]
pub struct Driver {
    /// Unique driver ID.
    pub id: u32,
    /// Driver name.
    pub name: String,
    /// Bus type this driver handles.
    pub bus_type: BusType,
    /// Device ID this driver is designed for (0 = any).
    pub device_id: u32,
    /// Whether the driver has been probed successfully.
    pub probed: bool,
}

/// Callback type for driver probe (returns true on success).
pub type ProbeCallback = fn(device_id: u32) -> bool;

/// Callback type for driver remove.
pub type RemoveCallback = fn(device_id: u32);

/// Callback type for suspend/resume.
pub type SuspendCallback = fn(device_id: u32) -> bool;
pub type ResumeCallback = fn(device_id: u32) -> bool;

/// Stored callbacks for a driver.
struct DriverCallbacks {
    probe: Option<ProbeCallback>,
    remove: Option<RemoveCallback>,
    suspend: Option<SuspendCallback>,
    resume: Option<ResumeCallback>,
}

/// Global driver registry.
struct DriverModel {
    devices: Vec<Device>,
    drivers: Vec<Driver>,
    callbacks: Vec<DriverCallbacks>,
    next_device_id: u32,
    next_driver_id: u32,
}

static MODEL: Mutex<Option<DriverModel>> = Mutex::new(None);

/// Initialize the driver model.
pub fn init_driver_model() {
    *MODEL.lock() = Some(DriverModel {
        devices: Vec::new(),
        drivers: Vec::new(),
        callbacks: Vec::new(),
        next_device_id: 1,
        next_driver_id: 1,
    });
}

/// Reset the driver model (for tests).
pub fn reset_driver_model() {
    *MODEL.lock() = None;
}

/// Register a device. Returns the device ID.
pub fn register_device(
    name: &str,
    bus_type: BusType,
    capabilities: u32,
) -> u32 {
    let mut model = MODEL.lock();
    let m = model.as_mut().expect("driver model not initialized");
    let id = m.next_device_id;
    m.next_device_id += 1;

    m.devices.push(Device {
        id,
        name: String::from(name),
        bus_type,
        power_state: DevicePowerState::Active,
        capabilities,
        ref_count: 0,
        enabled: true,
        driver_id: None,
        private_data: 0,
    });

    id
}

/// Unregister a device. Returns true if found and removed.
pub fn unregister_device(device_id: u32) -> bool {
    let mut model = MODEL.lock();
    let m = model.as_mut().expect("driver model not initialized");
    let len_before = m.devices.len();
    m.devices.retain(|d| d.id != device_id);
    m.devices.len() < len_before
}

/// Register a driver with callbacks. Returns the driver ID.
pub fn register_driver(
    name: &str,
    bus_type: BusType,
    device_id: u32,
    probe: Option<ProbeCallback>,
    remove: Option<RemoveCallback>,
    suspend: Option<SuspendCallback>,
    resume: Option<ResumeCallback>,
) -> u32 {
    let mut model = MODEL.lock();
    let m = model.as_mut().expect("driver model not initialized");
    let id = m.next_driver_id;
    m.next_driver_id += 1;

    m.drivers.push(Driver {
        id,
        name: String::from(name),
        bus_type,
        device_id,
        probed: false,
    });

    m.callbacks.push(DriverCallbacks {
        probe,
        remove,
        suspend,
        resume,
    });

    id
}

/// Probe a device with all matching drivers. Returns true if a driver bound.
pub fn probe_device(device_id: u32) -> bool {
    let mut model = MODEL.lock();
    let m = model.as_mut().expect("driver model not initialized");

    let device_idx = match m.devices.iter().position(|d| d.id == device_id) {
        Some(i) => i,
        None => return false,
    };

    let device_bus = m.devices[device_idx].bus_type;

    // Find matching drivers
    let driver_ids: Vec<u32> = m.drivers
        .iter()
        .filter(|d| d.bus_type == device_bus && (d.device_id == 0 || d.device_id == device_id))
        .map(|d| d.id)
        .collect();

    for driver_id in driver_ids {
        let driver_idx = match m.drivers.iter().position(|d| d.id == driver_id) {
            Some(i) => i,
            None => continue,
        };

        if let Some(probe_fn) = m.callbacks[driver_idx].probe {
            if probe_fn(device_id) {
                m.drivers[driver_idx].probed = true;
                m.devices[device_idx].driver_id = Some(driver_id);
                return true;
            }
        }
    }

    false
}

/// Remove the driver bound to a device.
pub fn remove_driver(device_id: u32) -> bool {
    let mut model = MODEL.lock();
    let m = model.as_mut().expect("driver model not initialized");

    let device_idx = match m.devices.iter().position(|d| d.id == device_id) {
        Some(i) => i,
        None => return false,
    };

    let driver_id = match m.devices[device_idx].driver_id {
        Some(id) => id,
        None => return false,
    };

    let driver_idx = match m.drivers.iter().position(|d| d.id == driver_id) {
        Some(i) => i,
        None => return false,
    };

    if let Some(remove_fn) = m.callbacks[driver_idx].remove {
        remove_fn(device_id);
    }

    m.drivers[driver_idx].probed = false;
    m.devices[device_idx].driver_id = None;
    true
}

/// Suspend a device. Returns true on success.
pub fn suspend_device(device_id: u32) -> bool {
    let mut model = MODEL.lock();
    let m = model.as_mut().expect("driver model not initialized");

    let device_idx = match m.devices.iter().position(|d| d.id == device_id) {
        Some(i) => i,
        None => return false,
    };

    if m.devices[device_idx].power_state == DevicePowerState::SystemSuspended {
        return false; // Already suspended
    }

    m.devices[device_idx].power_state = DevicePowerState::Suspending;

    let driver_id = m.devices[device_idx].driver_id;
    if let Some(did) = driver_id {
        if let Some(driver_idx) = m.drivers.iter().position(|d| d.id == did) {
            if let Some(suspend_fn) = m.callbacks[driver_idx].suspend {
                if !suspend_fn(device_id) {
                    m.devices[device_idx].power_state = DevicePowerState::Active;
                    return false;
                }
            }
        }
    }

    m.devices[device_idx].power_state = DevicePowerState::SystemSuspended;
    true
}

/// Resume a device. Returns true on success.
pub fn resume_device(device_id: u32) -> bool {
    let mut model = MODEL.lock();
    let m = model.as_mut().expect("driver model not initialized");

    let device_idx = match m.devices.iter().position(|d| d.id == device_id) {
        Some(i) => i,
        None => return false,
    };

    if m.devices[device_idx].power_state == DevicePowerState::Active {
        return false; // Already active
    }

    m.devices[device_idx].power_state = DevicePowerState::Resuming;

    let driver_id = m.devices[device_idx].driver_id;
    if let Some(did) = driver_id {
        if let Some(driver_idx) = m.drivers.iter().position(|d| d.id == did) {
            if let Some(resume_fn) = m.callbacks[driver_idx].resume {
                if !resume_fn(device_id) {
                    m.devices[device_idx].power_state = DevicePowerState::SystemSuspended;
                    return false;
                }
            }
        }
    }

    m.devices[device_idx].power_state = DevicePowerState::Active;
    true
}

/// Suspend all devices.
pub fn suspend_all_devices() -> u32 {
    let ids: Vec<u32> = MODEL.lock().as_ref()
        .map(|m| m.devices.iter().map(|d| d.id).collect())
        .unwrap_or_default();

    let mut suspended = 0;
    for id in ids {
        if suspend_device(id) {
            suspended += 1;
        }
    }
    suspended
}

/// Resume all devices.
pub fn resume_all_devices() -> u32 {
    let ids: Vec<u32> = MODEL.lock().as_ref()
        .map(|m| m.devices.iter().map(|d| d.id).collect())
        .unwrap_or_default();

    let mut resumed = 0;
    for id in ids {
        if resume_device(id) {
            resumed += 1;
        }
    }
    resumed
}

/// Get device info.
pub fn get_device(device_id: u32) -> Option<(String, BusType, DevicePowerState, u32)> {
    let model = MODEL.lock();
    let m = model.as_ref()?;
    m.devices.iter().find(|d| d.id == device_id).map(|d| {
        (d.name.clone(), d.bus_type, d.power_state, d.ref_count)
    })
}

/// Get driver info.
pub fn get_driver(driver_id: u32) -> Option<(String, BusType, bool)> {
    let model = MODEL.lock();
    let m = model.as_ref()?;
    m.drivers.iter().find(|d| d.id == driver_id).map(|d| {
        (d.name.clone(), d.bus_type, d.probed)
    })
}

/// List all devices.
pub fn list_devices() -> Vec<(u32, String, BusType, DevicePowerState)> {
    let model = MODEL.lock();
    let m = match model.as_ref() {
        Some(m) => m,
        None => return Vec::new(),
    };
    m.devices.iter().map(|d| {
        (d.id, d.name.clone(), d.bus_type, d.power_state)
    }).collect()
}

/// List all drivers.
pub fn list_drivers() -> Vec<(u32, String, BusType, bool)> {
    let model = MODEL.lock();
    let m = match model.as_ref() {
        Some(m) => m,
        None => return Vec::new(),
    };
    m.drivers.iter().map(|d| {
        (d.id, d.name.clone(), d.bus_type, d.probed)
    }).collect()
}

/// Get total device count.
pub fn device_count() -> usize {
    MODEL.lock().as_ref().map(|m| m.devices.len()).unwrap_or(0)
}

/// Get total driver count.
pub fn driver_count() -> usize {
    MODEL.lock().as_ref().map(|m| m.drivers.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_probe(_device_id: u32) -> bool { true }
    fn test_remove(_device_id: u32) {}
    fn test_suspend(_device_id: u32) -> bool { true }
    fn test_resume(_device_id: u32) -> bool { true }
    fn failing_probe(_device_id: u32) -> bool { false }

    #[test]
    fn init_and_register_device() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        let id = register_device("test_dev", BusType::Pci, DEV_CAP_DMA);
        assert_eq!(id, 1);
        assert_eq!(device_count(), 1);
    }

    #[test]
    fn register_multiple_devices() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        let id1 = register_device("dev1", BusType::Pci, 0);
        let id2 = register_device("dev2", BusType::Usb, 0);
        assert_ne!(id1, id2);
        assert_eq!(device_count(), 2);
    }

    #[test]
    fn test_unregister_device() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        let id = register_device("removable", BusType::Pci, 0);
        assert!(unregister_device(id));
        assert!(!unregister_device(id));
        assert_eq!(device_count(), 0);
    }

    #[test]
    fn register_and_probe_driver() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        let dev_id = register_device("pci_dev", BusType::Pci, 0);
        let _drv_id = register_driver(
            "test_driver", BusType::Pci, 0,
            Some(test_probe), None, None, None,
        );
        assert!(probe_device(dev_id));
        let (name, _, state, _) = get_device(dev_id).unwrap();
        assert_eq!(name, "pci_dev");
        assert_eq!(state, DevicePowerState::Active);
    }

    #[test]
    fn probe_fails_no_matching_driver() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        let dev_id = register_device("dev", BusType::Usb, 0);
        let _drv_id = register_driver(
            "pci_driver", BusType::Pci, 0,
            Some(test_probe), None, None, None,
        );
        assert!(!probe_device(dev_id));
    }

    #[test]
    fn probe_failing_driver() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        let dev_id = register_device("dev", BusType::Pci, 0);
        let _drv_id = register_driver(
            "bad_driver", BusType::Pci, 0,
            Some(failing_probe), None, None, None,
        );
        assert!(!probe_device(dev_id));
    }

    #[test]
    fn remove_driver_unbinds() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        let dev_id = register_device("dev", BusType::Pci, 0);
        let _drv_id = register_driver(
            "drv", BusType::Pci, 0,
            Some(test_probe), Some(test_remove), None, None,
        );
        probe_device(dev_id);
        assert!(remove_driver(dev_id));
        let (_, _, state, _) = get_device(dev_id).unwrap();
        assert_eq!(state, DevicePowerState::Active);
    }

    #[test]
    fn suspend_and_resume() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        let dev_id = register_device("dev", BusType::Pci, 0);
        let _drv_id = register_driver(
            "drv", BusType::Pci, 0,
            Some(test_probe), None,
            Some(test_suspend), Some(test_resume),
        );
        probe_device(dev_id);

        assert!(suspend_device(dev_id));
        let (_, _, state, _) = get_device(dev_id).unwrap();
        assert_eq!(state, DevicePowerState::SystemSuspended);

        assert!(resume_device(dev_id));
        let (_, _, state, _) = get_device(dev_id).unwrap();
        assert_eq!(state, DevicePowerState::Active);
    }

    #[test]
    fn suspend_all_and_resume_all() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        let id1 = register_device("d1", BusType::Pci, 0);
        let id2 = register_device("d2", BusType::Pci, 0);
        let _drv = register_driver(
            "drv", BusType::Pci, 0,
            Some(test_probe), None,
            Some(test_suspend), Some(test_resume),
        );
        probe_device(id1);
        probe_device(id2);

        let suspended = suspend_all_devices();
        assert_eq!(suspended, 2);

        let resumed = resume_all_devices();
        assert_eq!(resumed, 2);
    }

    #[test]
    fn list_devices_and_drivers() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        register_device("d1", BusType::Pci, 0);
        register_device("d2", BusType::Usb, 0);
        register_driver("drv1", BusType::Pci, 0, None, None, None, None);

        assert_eq!(list_devices().len(), 2);
        assert_eq!(list_drivers().len(), 1);
    }

    #[test]
    fn device_capabilities() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        let id = register_device("cap_dev", BusType::Pci, DEV_CAP_DMA | DEV_CAP_IRQ);
        let (_, _, _, _) = get_device(id).unwrap();
        assert_eq!(device_count(), 1);
    }

    #[test]
    fn non_device_id_driver_matches_any() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        let dev_id = register_device("dev", BusType::Virtio, 0);
        let _drv_id = register_driver(
            "virtio_driver", BusType::Virtio, 0, // device_id=0 means any
            Some(test_probe), None, None, None,
        );
        assert!(probe_device(dev_id));
    }

    #[test]
    fn already_suspended_device_fails() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        let dev_id = register_device("dev", BusType::Pci, 0);
        let _drv = register_driver(
            "drv", BusType::Pci, 0,
            Some(test_probe), None,
            Some(test_suspend), Some(test_resume),
        );
        probe_device(dev_id);
        suspend_device(dev_id);
        assert!(!suspend_device(dev_id)); // already suspended
    }

    #[test]
    fn already_active_device_resume_fails() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        let dev_id = register_device("dev", BusType::Pci, 0);
        let _drv = register_driver(
            "drv", BusType::Pci, 0,
            Some(test_probe), None, None,
            Some(test_resume),
        );
        probe_device(dev_id);
        assert!(!resume_device(dev_id)); // already active
    }

    #[test]
    fn unprobed_device_remove_fails() {
        let _s = crate::test_serial::acquire();
        reset_driver_model();
        init_driver_model();
        let dev_id = register_device("dev", BusType::Pci, 0);
        assert!(!remove_driver(dev_id)); // no driver bound
    }
}
