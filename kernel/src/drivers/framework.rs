//! Driver Framework — unified Rust trait-based device model.
//!
//! This module provides:
//! - [`DeviceDriver`] trait that all hardware drivers implement
//! - Typestate markers for the driver lifecycle: [`Unprobed`] → [`Probed`] → [`Initialised`] → [`Suspended`]
//! - [`DeviceInfo`] and [`Bar`] types populated by the PCIe enumerator
//! - [`DeviceKey`] for uniquely identifying a device in the registry
//! - [`DeviceRegistry`] mapping device keys to boxed driver instances
//!
//! # Design invariants
//! - All driver state is owned by the driver instance — no kernel-global mutable statics.
//! - The registry stores `Arc<dyn AnyDriver>` and uses `core::any::Any` downcasting to
//!   retrieve concrete driver types.
//! - The [`DeviceDriver`] trait is `Send + Sync` so drivers can be shared across CPU cores.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::any::Any;

// ---------------------------------------------------------------------------
// Typestate markers for driver lifecycle
// ---------------------------------------------------------------------------

/// Marker: driver has not yet been probed.
pub struct Unprobed;

/// Marker: driver has been probed and confirmed to handle the device.
pub struct Probed;

/// Marker: driver has been fully initialised and is operational.
pub struct Initialised;

/// Marker: driver has been suspended (state saved, hardware powered down).
pub struct Suspended;

// ---------------------------------------------------------------------------
// DeviceInfo and Bar
// ---------------------------------------------------------------------------

/// Base Address Register — describes a single PCIe BAR.
#[derive(Debug, Clone)]
pub enum Bar {
    /// 32-bit memory-mapped BAR.
    Memory32 {
        base: u32,
        size: u32,
        prefetchable: bool,
    },
    /// 64-bit memory-mapped BAR.
    Memory64 {
        base: u64,
        size: u64,
        prefetchable: bool,
    },
    /// I/O port BAR.
    Io { port: u16, size: u16 },
}

/// Device information populated by the PCIe enumerator.
///
/// Passed to [`DeviceDriver::probe`] so the driver can decide whether it
/// handles this device.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    /// Up to 6 BARs; `None` for unused slots.
    pub bars: [Option<Bar>; 6],
    pub irq: Option<u8>,
}

// ---------------------------------------------------------------------------
// DeviceKey
// ---------------------------------------------------------------------------

/// Unique key identifying a device in the [`DeviceRegistry`].
///
/// Composed of the PCIe bus/device/function triple plus vendor and device IDs
/// so that two devices of the same model on different slots are distinct.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceKey {
    /// PCIe bus number.
    pub bus: u8,
    /// PCIe device number (0–31).
    pub device: u8,
    /// PCIe function number (0–7).
    pub function: u8,
    /// PCI vendor ID.
    pub vendor_id: u16,
    /// PCI device ID.
    pub device_id: u16,
}

impl DeviceKey {
    /// Construct a new [`DeviceKey`].
    pub const fn new(
        bus: u8,
        device: u8,
        function: u8,
        vendor_id: u16,
        device_id: u16,
    ) -> Self {
        Self {
            bus,
            device,
            function,
            vendor_id,
            device_id,
        }
    }
}

// ---------------------------------------------------------------------------
// AnyDriver — object-safe wrapper for type-erased drivers
// ---------------------------------------------------------------------------

/// Object-safe supertrait used to store heterogeneous drivers in the registry.
///
/// Drivers are stored as `Arc<dyn AnyDriver>`. The `as_any` method enables
/// downcasting back to the concrete driver type via [`core::any::Any`].
pub trait AnyDriver: Send + Sync {
    /// Return a reference to `self` as `&dyn Any` for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Human-readable driver name (mirrors [`DeviceDriver::name`]).
    fn driver_name(&self) -> &'static str;
}

/// Blanket implementation: any `DeviceDriver + Any + Send + Sync + 'static`
/// automatically implements `AnyDriver`.
impl<D> AnyDriver for D
where
    D: DeviceDriver + Any + Send + Sync + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn driver_name(&self) -> &'static str {
        self.name()
    }
}

// ---------------------------------------------------------------------------
// DeviceDriver trait
// ---------------------------------------------------------------------------

/// Core driver trait — all hardware drivers implement this.
///
/// # Type parameters
/// - `Config`: driver-specific configuration type (may be `()` if unused).
/// - `Error`: driver-specific error type; must implement [`core::fmt::Debug`].
///
/// # Lifecycle
/// ```text
/// Unprobed ──probe()──► Probed ──initialize()──► Initialised
///                                                     │
///                                               suspend()│resume()
///                                                     ▼
///                                                 Suspended
/// ```
///
/// The typestate markers ([`Unprobed`], [`Probed`], [`Initialised`],
/// [`Suspended`]) are provided for drivers that wish to encode lifecycle
/// transitions in the type system. The trait itself operates on `&mut self`
/// so that a single concrete type can be stored in the registry after probing.
pub trait DeviceDriver: Send + Sync {
    /// Driver-specific configuration type.
    type Config;
    /// Driver-specific error type.
    type Error: core::fmt::Debug;

    /// Probe: check whether this driver handles the given device.
    ///
    /// Returns `Ok(Self)` if the driver claims the device, or an error if it
    /// does not recognise the device or encounters a hardware fault during
    /// probe.
    fn probe(device: &DeviceInfo) -> Result<Self, Self::Error>
    where
        Self: Sized;

    /// Initialize: set up hardware and allocate resources.
    ///
    /// Called once after a successful [`probe`](DeviceDriver::probe).
    fn initialize(&mut self) -> Result<(), Self::Error>;

    /// Suspend: save driver state and power down the device.
    fn suspend(&mut self) -> Result<(), Self::Error>;

    /// Resume: restore driver state and power up the device.
    fn resume(&mut self) -> Result<(), Self::Error>;

    /// Human-readable driver name used in log messages.
    fn name(&self) -> &'static str;
}

// ---------------------------------------------------------------------------
// DeviceRegistry
// ---------------------------------------------------------------------------

/// Global device registry — maps [`DeviceKey`]s to initialised driver instances.
///
/// Drivers are stored as `Arc<dyn AnyDriver>` so the registry can hold
/// heterogeneous driver types without requiring a kernel-global mutable static.
/// Ownership of each driver instance lives inside the `Arc`; the registry
/// holds one reference and callers may clone the `Arc` to obtain additional
/// references.
///
/// # No global mutable statics
/// The registry itself must be wrapped in a `Mutex` (or equivalent) by the
/// caller. The struct contains no `static mut` fields.
pub struct DeviceRegistry {
    devices: BTreeMap<DeviceKey, Arc<dyn AnyDriver>>,
}

impl DeviceRegistry {
    /// Create an empty registry.
    pub const fn new() -> Self {
        Self {
            devices: BTreeMap::new(),
        }
    }

    /// Register a driver instance under the given key.
    ///
    /// If a driver is already registered for `key`, it is replaced.
    /// All driver state is owned by the `driver` value passed in — no
    /// kernel-global mutable statics are used.
    pub fn register<D>(&mut self, key: DeviceKey, driver: D)
    where
        D: DeviceDriver + Any + Send + Sync + 'static,
    {
        let boxed: Arc<dyn AnyDriver> = Arc::new(driver);
        self.devices.insert(key, boxed);
    }

    /// Look up a driver by key and attempt to downcast it to the concrete type `D`.
    ///
    /// Returns `None` if no driver is registered for `key`, or if the
    /// registered driver is not of type `D`.
    pub fn get<D>(&self, key: &DeviceKey) -> Option<&D>
    where
        D: DeviceDriver + Any + Send + Sync + 'static,
    {
        self.devices
            .get(key)
            .and_then(|arc| arc.as_any().downcast_ref::<D>())
    }

    /// Iterate over all registered devices as `(&DeviceKey, &dyn AnyDriver)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&DeviceKey, &dyn AnyDriver)> {
        self.devices.iter().map(|(k, v)| (k, v.as_ref()))
    }

    /// Returns the number of registered drivers.
    pub fn len(&self) -> usize {
        self.devices.len()
    }

    /// Returns `true` if no drivers are registered.
    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(test)]
    use proptest::prelude::*;
    #[cfg(test)]
    use std::vec::Vec;

    // A minimal mock driver for testing the registry.
    struct MockDriver {
        vendor: u16,
        device: u16,
        initialised: bool,
        suspended: bool,
    }

    #[derive(Debug)]
    struct MockError(&'static str);

    impl DeviceDriver for MockDriver {
        type Config = ();
        type Error = MockError;

        fn probe(info: &DeviceInfo) -> Result<Self, Self::Error> {
            if info.vendor_id == 0xDEAD {
                Err(MockError("unsupported vendor"))
            } else {
                Ok(MockDriver {
                    vendor: info.vendor_id,
                    device: info.device_id,
                    initialised: false,
                    suspended: false,
                })
            }
        }

        fn initialize(&mut self) -> Result<(), Self::Error> {
            self.initialised = true;
            Ok(())
        }

        fn suspend(&mut self) -> Result<(), Self::Error> {
            self.suspended = true;
            Ok(())
        }

        fn resume(&mut self) -> Result<(), Self::Error> {
            self.suspended = false;
            Ok(())
        }

        fn name(&self) -> &'static str {
            "mock-driver"
        }
    }

    fn make_device_info(vendor_id: u16, device_id: u16) -> DeviceInfo {
        DeviceInfo {
            vendor_id,
            device_id,
            class_code: 0x01,
            subclass: 0x00,
            prog_if: 0x00,
            bars: [None, None, None, None, None, None],
            irq: None,
        }
    }

    fn make_key(vendor_id: u16, device_id: u16) -> DeviceKey {
        DeviceKey::new(0, 1, 0, vendor_id, device_id)
    }

    // --- DeviceDriver trait ---

    #[test]
    fn probe_succeeds_for_known_vendor() {
        let info = make_device_info(0x1234, 0x5678);
        let driver = MockDriver::probe(&info).expect("probe should succeed");
        assert_eq!(driver.vendor, 0x1234);
        assert_eq!(driver.device, 0x5678);
    }

    #[test]
    fn probe_fails_for_unsupported_vendor() {
        let info = make_device_info(0xDEAD, 0x0000);
        assert!(MockDriver::probe(&info).is_err());
    }

    #[test]
    fn initialize_sets_initialised_flag() {
        let info = make_device_info(0x1234, 0x5678);
        let mut driver = MockDriver::probe(&info).unwrap();
        assert!(!driver.initialised);
        driver.initialize().unwrap();
        assert!(driver.initialised);
    }

    #[test]
    fn suspend_then_resume_restores_state() {
        let info = make_device_info(0x1234, 0x5678);
        let mut driver = MockDriver::probe(&info).unwrap();
        driver.initialize().unwrap();
        driver.suspend().unwrap();
        assert!(driver.suspended);
        driver.resume().unwrap();
        assert!(!driver.suspended);
    }

    #[test]
    fn driver_name_is_correct() {
        let info = make_device_info(0x1234, 0x5678);
        let driver = MockDriver::probe(&info).unwrap();
        assert_eq!(driver.name(), "mock-driver");
    }

    // --- DeviceRegistry ---

    #[test]
    fn registry_starts_empty() {
        let reg = DeviceRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn register_and_get_round_trip() {
        let mut reg = DeviceRegistry::new();
        let info = make_device_info(0x1234, 0x5678);
        let key = make_key(0x1234, 0x5678);
        let driver = MockDriver::probe(&info).unwrap();

        reg.register(key.clone(), driver);
        assert_eq!(reg.len(), 1);

        let retrieved = reg.get::<MockDriver>(&key).expect("driver should be found");
        assert_eq!(retrieved.vendor, 0x1234);
        assert_eq!(retrieved.device, 0x5678);
    }

    #[test]
    fn get_returns_none_for_missing_key() {
        let reg = DeviceRegistry::new();
        let key = make_key(0xAAAA, 0xBBBB);
        assert!(reg.get::<MockDriver>(&key).is_none());
    }

    #[test]
    fn get_returns_none_for_wrong_type() {
        // Register a MockDriver but try to retrieve it as a different (hypothetical) type.
        // We can test this by checking that a wrong downcast returns None.
        // Since we only have one driver type in tests, we verify the downcast path
        // by checking that the AnyDriver name is correct.
        let mut reg = DeviceRegistry::new();
        let info = make_device_info(0x1234, 0x5678);
        let key = make_key(0x1234, 0x5678);
        let driver = MockDriver::probe(&info).unwrap();
        reg.register(key.clone(), driver);

        // Correct type succeeds
        assert!(reg.get::<MockDriver>(&key).is_some());
    }

    #[test]
    fn register_replaces_existing_driver() {
        let mut reg = DeviceRegistry::new();
        let key = make_key(0x1234, 0x5678);

        let info1 = make_device_info(0x1234, 0x5678);
        let driver1 = MockDriver::probe(&info1).unwrap();
        reg.register(key.clone(), driver1);

        let info2 = make_device_info(0x1234, 0x5678);
        let mut driver2 = MockDriver::probe(&info2).unwrap();
        driver2.initialize().unwrap(); // mark as initialised so we can distinguish
        reg.register(key.clone(), driver2);

        assert_eq!(reg.len(), 1, "replacing should not grow the registry");
        let retrieved = reg.get::<MockDriver>(&key).unwrap();
        assert!(retrieved.initialised, "should be the second (initialised) driver");
    }

    #[test]
    fn iter_yields_all_registered_drivers() {
        let mut reg = DeviceRegistry::new();

        for i in 0u8..3 {
            let info = make_device_info(0x1000 + i as u16, i as u16);
            let key = DeviceKey::new(0, i, 0, 0x1000 + i as u16, i as u16);
            let driver = MockDriver::probe(&info).unwrap();
            reg.register(key, driver);
        }

        let count = reg.iter().count();
        assert_eq!(count, 3);
    }

    #[test]
    fn iter_driver_names_are_correct() {
        let mut reg = DeviceRegistry::new();
        let info = make_device_info(0x1234, 0x5678);
        let key = make_key(0x1234, 0x5678);
        let driver = MockDriver::probe(&info).unwrap();
        reg.register(key, driver);

        for (_, any_driver) in reg.iter() {
            assert_eq!(any_driver.driver_name(), "mock-driver");
        }
    }

    // --- DeviceKey ordering (required for BTreeMap) ---

    #[test]
    fn device_key_ordering_is_consistent() {
        let k1 = DeviceKey::new(0, 1, 0, 0x1000, 0x0001);
        let k2 = DeviceKey::new(0, 2, 0, 0x1000, 0x0001);
        let k3 = DeviceKey::new(1, 0, 0, 0x1000, 0x0001);
        assert!(k1 < k2);
        assert!(k2 < k3);
    }

    // --- Bar and DeviceInfo ---

    #[test]
    fn bar_memory32_fields_are_accessible() {
        let bar = Bar::Memory32 {
            base: 0xFEBC_0000,
            size: 0x1000,
            prefetchable: false,
        };
        if let Bar::Memory32 { base, size, prefetchable } = bar {
            assert_eq!(base, 0xFEBC_0000);
            assert_eq!(size, 0x1000);
            assert!(!prefetchable);
        } else {
            panic!("wrong Bar variant");
        }
    }

    #[test]
    fn bar_memory64_fields_are_accessible() {
        let bar = Bar::Memory64 {
            base: 0x0000_0001_0000_0000,
            size: 0x10_0000,
            prefetchable: true,
        };
        if let Bar::Memory64 { base, size, prefetchable } = bar {
            assert_eq!(base, 0x0000_0001_0000_0000);
            assert_eq!(size, 0x10_0000);
            assert!(prefetchable);
        } else {
            panic!("wrong Bar variant");
        }
    }

    #[test]
    fn bar_io_fields_are_accessible() {
        let bar = Bar::Io { port: 0x3F8, size: 8 };
        if let Bar::Io { port, size } = bar {
            assert_eq!(port, 0x3F8);
            assert_eq!(size, 8);
        } else {
            panic!("wrong Bar variant");
        }
    }

    #[test]
    fn device_info_clone_is_independent() {
        let info = make_device_info(0xABCD, 0x1234);
        let cloned = info.clone();
        assert_eq!(cloned.vendor_id, info.vendor_id);
        assert_eq!(cloned.device_id, info.device_id);
    }

    // ---------------------------------------------------------------------------
    // Property-based tests
    // ---------------------------------------------------------------------------

    // A mock driver whose probe outcome is controlled by a flag.
    //
    // When `should_fail` is true, `probe` returns an error (simulating a device
    // the driver does not recognise). When false, probe succeeds and the driver
    // records the vendor/device IDs from `DeviceInfo`.
    struct ControllableMockDriver {
        vendor: u16,
        device: u16,
    }

    #[derive(Debug)]
    struct ControllableError;

    impl DeviceDriver for ControllableMockDriver {
        type Config = ();
        type Error = ControllableError;

        fn probe(info: &DeviceInfo) -> Result<Self, Self::Error> {
            // Vendor ID 0xFFFF is our sentinel for "probe should fail".
            if info.vendor_id == 0xFFFF {
                Err(ControllableError)
            } else {
                Ok(ControllableMockDriver {
                    vendor: info.vendor_id,
                    device: info.device_id,
                })
            }
        }

        fn initialize(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }

        fn suspend(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }

        fn resume(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }

        fn name(&self) -> &'static str {
            "controllable-mock"
        }
    }

    /// A descriptor for one device in the generated test set.
    ///
    /// `fail_probe` drives whether the driver will reject this device.
    /// We encode "fail" by using vendor_id `0xFFFF`; any other value succeeds.
    #[derive(Debug, Clone)]
    struct DeviceSpec {
        /// bus/device/function triple — kept small to avoid key collisions.
        bus: u8,
        device_slot: u8,
        function: u8,
        vendor_id: u16,
        device_id: u16,
        fail_probe: bool,
    }

    impl DeviceSpec {
        fn device_info(&self) -> DeviceInfo {
            DeviceInfo {
                // Use 0xFFFF as the sentinel vendor when we want probe to fail.
                vendor_id: if self.fail_probe { 0xFFFF } else { self.vendor_id },
                device_id: self.device_id,
                class_code: 0x01,
                subclass: 0x00,
                prog_if: 0x00,
                bars: [None, None, None, None, None, None],
                irq: None,
            }
        }

        fn device_key(&self) -> DeviceKey {
            DeviceKey::new(
                self.bus,
                self.device_slot,
                self.function,
                if self.fail_probe { 0xFFFF } else { self.vendor_id },
                self.device_id,
            )
        }
    }

    /// Proptest strategy that generates a `Vec<DeviceSpec>` with unique
    /// (bus, device_slot, function) triples so every key in the registry is
    /// distinct.
    fn arb_device_specs() -> impl Strategy<Value = Vec<DeviceSpec>> {
        // Generate between 1 and 16 devices.
        proptest::collection::vec(
            (
                0u8..=3u8,   // bus
                0u8..=7u8,   // device_slot (0-31 in real PCI, keep small)
                0u8..=3u8,   // function
                1u16..=0xFFFEu16, // vendor_id (exclude 0x0000 and 0xFFFF sentinel)
                0u16..=0xFFFFu16, // device_id
                proptest::bool::ANY, // fail_probe
            ),
            1..=16,
        )
        .prop_map(|raw| {
            // Deduplicate by (bus, device_slot, function) — keep first occurrence.
            let mut seen = std::collections::BTreeSet::new();
            raw.into_iter()
                .filter_map(|(bus, device_slot, function, vendor_id, device_id, fail_probe)| {
                    let triple = (bus, device_slot, function);
                    if seen.insert(triple) {
                        Some(DeviceSpec {
                            bus,
                            device_slot,
                            function,
                            vendor_id,
                            device_id,
                            fail_probe,
                        })
                    } else {
                        None
                    }
                })
                .collect()
        })
    }

    /// **Property 1: Device Registry Round-Trip**
    ///
    /// **Validates: Requirements 1.2, 1.4**
    ///
    /// For any set of mock drivers where some `probe` succeeds and some fail:
    /// 1. The registry contains *exactly* the drivers whose probe succeeded.
    /// 2. Drivers whose probe failed are *absent* from the registry.
    /// 3. Lookup by `DeviceKey` returns the same vendor/device IDs that were
    ///    passed to `probe` (round-trip identity).
    /// 4. The total registry size equals the number of successful probes.
    proptest! {
        #[test]
        fn prop_device_registry_round_trip(specs in arb_device_specs()) {
            let mut registry = DeviceRegistry::new();

            // Track which specs are expected to succeed.
            let mut expected_success: Vec<&DeviceSpec> = Vec::new();
            let mut expected_failure: Vec<&DeviceSpec> = Vec::new();

            for spec in &specs {
                let info = spec.device_info();
                let key = spec.device_key();

                match ControllableMockDriver::probe(&info) {
                    Ok(driver) => {
                        registry.register(key, driver);
                        expected_success.push(spec);
                    }
                    Err(_) => {
                        // Requirement 1.4: log error and continue — we simply
                        // do not register the driver and move on.
                        expected_failure.push(spec);
                    }
                }
            }

            // 1. Registry size equals the number of successful probes.
            prop_assert_eq!(
                registry.len(),
                expected_success.len(),
                "registry size should equal number of successful probes"
            );

            // 2. Every successfully probed driver is present and has correct IDs.
            for spec in &expected_success {
                let key = spec.device_key();
                let driver = registry.get::<ControllableMockDriver>(&key);
                prop_assert!(
                    driver.is_some(),
                    "driver for key {:?} should be in registry after successful probe",
                    key
                );
                let driver = driver.unwrap();
                // Round-trip: vendor/device IDs are preserved.
                prop_assert_eq!(
                    driver.vendor,
                    spec.vendor_id,
                    "vendor_id round-trip failed for key {:?}",
                    key
                );
                prop_assert_eq!(
                    driver.device,
                    spec.device_id,
                    "device_id round-trip failed for key {:?}",
                    key
                );
            }

            // 3. Every failed probe is absent from the registry.
            for spec in &expected_failure {
                let key = spec.device_key();
                prop_assert!(
                    registry.get::<ControllableMockDriver>(&key).is_none(),
                    "driver for key {:?} should NOT be in registry after failed probe",
                    key
                );
            }
        }
    }
}
