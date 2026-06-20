use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceManagerError {
    HotplugError(String),
    DriverRuleError(String),
    MountError(String),
    IpcError(String),
    InvalidEvent(String),
}

impl std::fmt::Display for DeviceManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HotplugError(e) => write!(f, "hotplug error: {e}"),
            Self::DriverRuleError(e) => write!(f, "driver rule error: {e}"),
            Self::MountError(e) => write!(f, "mount error: {e}"),
            Self::IpcError(e) => write!(f, "IPC error: {e}"),
            Self::InvalidEvent(e) => write!(f, "invalid event: {e}"),
        }
    }
}

impl std::error::Error for DeviceManagerError {}

// ---------------------------------------------------------------------------
// Device types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceBus {
    Pci,
    Usb,
    Virtio,
    Unknown(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceId {
    pub vendor_id: u16,
    pub device_id: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub bus: DeviceBus,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u16,
    pub subclass_code: u8,
    pub prog_if: u8,
    pub bus_number: u8,
    pub device_number: u8,
    pub function_number: u8,
    pub description: String,
    pub driver: Option<String>,
    pub mount_point: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Hotplug event binary format
// ---------------------------------------------------------------------------
//
// Kernel delivers events over the hotplug_subscribe socket in this format:
//
//   u8   event_type   (1 = added, 2 = removed, 3 = changed)
//   u16  vendor_id    (little-endian)
//   u16  device_id    (little-endian)
//   u16  class_code   (little-endian)
//   u8   subclass_code
//   u8   prog_if
//   u8   bus          (PCI bus number)
//   u8   device       (PCI device number)
//   u8   function     (PCI function number)
//   u8   bus_type     (1 = PCI, 2 = USB, 3 = Virtio)
//   ... optional label (null-terminated) for USB storage
//

pub const HOTPLUG_EVENT_HEADER_SIZE: usize = 13;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotplugEventType {
    DeviceAdded,
    DeviceRemoved,
    DeviceChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawHotplugEvent {
    pub event_type: HotplugEventType,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u16,
    pub subclass_code: u8,
    pub prog_if: u8,
    pub bus_number: u8,
    pub device_number: u8,
    pub function_number: u8,
    pub bus_type: DeviceBus,
    pub label: Option<String>,
}

pub fn parse_hotplug_event(data: &[u8]) -> Result<RawHotplugEvent, DeviceManagerError> {
    if data.len() < HOTPLUG_EVENT_HEADER_SIZE {
        return Err(DeviceManagerError::InvalidEvent(format!(
            "event too short: {} bytes, need at least {HOTPLUG_EVENT_HEADER_SIZE}",
            data.len()
        )));
    }

    let event_type = match data[0] {
        1 => HotplugEventType::DeviceAdded,
        2 => HotplugEventType::DeviceRemoved,
        3 => HotplugEventType::DeviceChanged,
        t => return Err(DeviceManagerError::InvalidEvent(format!("unknown event type: {t}"))),
    };

    let vendor_id = u16::from_le_bytes([data[1], data[2]]);
    let device_id = u16::from_le_bytes([data[3], data[4]]);
    let class_code = u16::from_le_bytes([data[5], data[6]]);
    let subclass_code = data[7];
    let prog_if = data[8];
    let bus_number = data[9];
    let device_number = data[10];
    let function_number = data[11];
    let bus_type = match data[12] {
        1 => DeviceBus::Pci,
        2 => DeviceBus::Usb,
        3 => DeviceBus::Virtio,
        t => DeviceBus::Unknown(t),
    };

    let label = if data.len() > HOTPLUG_EVENT_HEADER_SIZE {
        let rest = &data[HOTPLUG_EVENT_HEADER_SIZE..];
        let null_pos = rest.iter().position(|&b| b == 0);
        null_pos.map(|pos| String::from_utf8_lossy(&rest[..pos]).to_string())
    } else {
        None
    };

    Ok(RawHotplugEvent {
        event_type,
        vendor_id,
        device_id,
        class_code,
        subclass_code,
        prog_if,
        bus_number,
        device_number,
        function_number,
        bus_type,
        label,
    })
}

pub fn serialize_hotplug_event(event: &RawHotplugEvent) -> Vec<u8> {
    let type_byte: u8 = match event.event_type {
        HotplugEventType::DeviceAdded => 1,
        HotplugEventType::DeviceRemoved => 2,
        HotplugEventType::DeviceChanged => 3,
    };
    let bus_byte: u8 = match event.bus_type {
        DeviceBus::Pci => 1,
        DeviceBus::Usb => 2,
        DeviceBus::Virtio => 3,
        DeviceBus::Unknown(t) => t,
    };

    let mut buf = Vec::with_capacity(64);
    buf.push(type_byte);
    buf.extend_from_slice(&event.vendor_id.to_le_bytes());
    buf.extend_from_slice(&event.device_id.to_le_bytes());
    buf.extend_from_slice(&event.class_code.to_le_bytes());
    buf.push(event.subclass_code);
    buf.push(event.prog_if);
    buf.push(event.bus_number);
    buf.push(event.device_number);
    buf.push(event.function_number);
    buf.push(bus_byte);

    if let Some(ref label) = event.label {
        buf.extend_from_slice(label.as_bytes());
        buf.push(0);
    }

    buf
}

// ---------------------------------------------------------------------------
// Driver rules
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverRule {
    pub vendor_id: u16,
    pub device_id: u16,
    pub driver_name: String,
    pub auto_probe: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DriverRuleTable {
    pub rules: Vec<DriverRule>,
}

impl DriverRuleTable {
    pub fn match_device(&self, vendor_id: u16, device_id: u16) -> Option<&DriverRule> {
        self.rules.iter().find(|r| r.vendor_id == vendor_id && r.device_id == device_id)
    }

    pub fn match_device_mut(&mut self, vendor_id: u16, device_id: u16) -> Option<&mut DriverRule> {
        self.rules.iter_mut().find(|r| r.vendor_id == vendor_id && r.device_id == device_id)
    }

    pub fn load_from_toml(path: &str) -> Result<Self, DeviceManagerError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| DeviceManagerError::DriverRuleError(format!("cannot read rules: {e}")))?;
        toml::from_str(&content)
            .map_err(|e| DeviceManagerError::DriverRuleError(format!("invalid rules TOML: {e}")))
    }
}

// Common driver rules
pub fn default_driver_rules() -> DriverRuleTable {
    DriverRuleTable {
        rules: vec![
            DriverRule {
                vendor_id: 0x8086,
                device_id: 0x100e,
                driver_name: "e1000".into(),
                auto_probe: true,
                description: "Intel PRO/1000 Network Adapter".into(),
            },
            DriverRule {
                vendor_id: 0x8086,
                device_id: 0x1237,
                driver_name: "pci-isa-bridge".into(),
                auto_probe: true,
                description: "Intel 82371SB PIIX3 ISA Bridge".into(),
            },
            DriverRule {
                vendor_id: 0x8086,
                device_id: 0x7000,
                driver_name: "pci-ide".into(),
                auto_probe: true,
                description: "Intel 82371SB PIIX3 IDE Controller".into(),
            },
            DriverRule {
                vendor_id: 0x1af4,
                device_id: 0x1000,
                driver_name: "virtio-net".into(),
                auto_probe: true,
                description: "VirtIO Network Device".into(),
            },
            DriverRule {
                vendor_id: 0x1af4,
                device_id: 0x1001,
                driver_name: "virtio-blk".into(),
                auto_probe: true,
                description: "VirtIO Block Device".into(),
            },
            DriverRule {
                vendor_id: 0x090c,
                device_id: 0x1000,
                driver_name: "usb-storage".into(),
                auto_probe: true,
                description: "Samsung Flash Drive USB Storage".into(),
            },
            DriverRule {
                vendor_id: 0x0781,
                device_id: 0x5583,
                driver_name: "usb-storage".into(),
                auto_probe: true,
                description: "SanDisk Ultra Fit USB Storage".into(),
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// Hotplug source trait
// ---------------------------------------------------------------------------

pub trait HotplugSource {
    fn poll_event(&mut self) -> Result<Option<RawHotplugEvent>, DeviceManagerError>;
}

// ---------------------------------------------------------------------------
// Mock hotplug source for testing
// ---------------------------------------------------------------------------

pub struct MockHotplugSource {
    events: Vec<RawHotplugEvent>,
}

impl MockHotplugSource {
    pub fn new(events: Vec<RawHotplugEvent>) -> Self {
        Self { events }
    }

    pub fn push(&mut self, event: RawHotplugEvent) {
        self.events.push(event);
    }
}

impl HotplugSource for MockHotplugSource {
    fn poll_event(&mut self) -> Result<Option<RawHotplugEvent>, DeviceManagerError> {
        if self.events.is_empty() {
            return Ok(None);
        }
        Ok(Some(self.events.remove(0)))
    }
}

// ---------------------------------------------------------------------------
// USB mount management
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountEntry {
    pub label: String,
    pub vendor_id: u16,
    pub device_id: u16,
    pub mount_path: PathBuf,
    pub is_mounted: bool,
}

impl MountEntry {
    pub fn new(label: String, vendor_id: u16, device_id: u16) -> Self {
        Self {
            mount_path: PathBuf::from(format!("/media/{label}")),
            label,
            vendor_id,
            device_id,
            is_mounted: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Device manager
// ---------------------------------------------------------------------------

pub struct DeviceManager {
    pub devices: Vec<DeviceInfo>,
    pub mounts: Vec<MountEntry>,
    pub driver_rules: DriverRuleTable,
    pub source: Box<dyn HotplugSource>,
    pub registered_name: Option<String>,
}

impl DeviceManager {
    pub fn new(source: Box<dyn HotplugSource>) -> Self {
        Self {
            devices: Vec::new(),
            mounts: Vec::new(),
            driver_rules: default_driver_rules(),
            source,
            registered_name: None,
        }
    }

    pub fn with_rules(source: Box<dyn HotplugSource>, rules: DriverRuleTable) -> Self {
        Self {
            devices: Vec::new(),
            mounts: Vec::new(),
            driver_rules: rules,
            source,
            registered_name: None,
        }
    }

    pub fn process_event(&mut self, event: &RawHotplugEvent) -> Result<(), DeviceManagerError> {
        match event.event_type {
            HotplugEventType::DeviceAdded => self.handle_device_added(event),
            HotplugEventType::DeviceRemoved => self.handle_device_removed(event),
            HotplugEventType::DeviceChanged => {
                // Re-check driver rules on change
                Ok(())
            }
        }
    }

    fn handle_device_added(&mut self, event: &RawHotplugEvent) -> Result<(), DeviceManagerError> {
        let driver_rule = self.driver_rules.match_device(event.vendor_id, event.device_id);
        let driver_name = driver_rule.map(|r| r.driver_name.clone());
        let description = driver_rule.map(|r| r.description.clone())
            .unwrap_or_else(|| format!("Unknown device {:04x}:{:04x}", event.vendor_id, event.device_id));

        let is_usb_storage = event.bus_type == DeviceBus::Usb
            && driver_name.as_deref() == Some("usb-storage");

        let mount_point = if is_usb_storage {
            let label = event.label.clone().unwrap_or_else(|| {
                format!("usb-{:04x}-{:04x}", event.vendor_id, event.device_id)
            });
            let mount_path = PathBuf::from(format!("/media/{label}"));

            // Create mount entry if not already tracked
            if !self.mounts.iter().any(|m| m.label == label) {
                let mut entry = MountEntry::new(label.clone(), event.vendor_id, event.device_id);
                entry.is_mounted = true;
                self.mounts.push(entry);
            }

            Some(mount_path)
        } else {
            None
        };

        self.devices.push(DeviceInfo {
            bus: event.bus_type,
            vendor_id: event.vendor_id,
            device_id: event.device_id,
            class_code: event.class_code,
            subclass_code: event.subclass_code,
            prog_if: event.prog_if,
            bus_number: event.bus_number,
            device_number: event.device_number,
            function_number: event.function_number,
            description,
            driver: driver_name,
            mount_point,
        });

        Ok(())
    }

    fn handle_device_removed(&mut self, event: &RawHotplugEvent) -> Result<(), DeviceManagerError> {
        self.devices.retain(|d| {
            !(d.vendor_id == event.vendor_id
                && d.device_id == event.device_id
                && d.bus_number == event.bus_number
                && d.device_number == event.device_number
                && d.function_number == event.function_number)
        });

        // Unmount USB storage
        let label = event.label.as_deref().unwrap_or("");
        if !label.is_empty() {
            if let Some(entry) = self.mounts.iter_mut().find(|m| m.label == label) {
                entry.is_mounted = false;
            }
        } else {
            // Match by vendor/device if no label
            for entry in self.mounts.iter_mut() {
                if entry.vendor_id == event.vendor_id && entry.device_id == event.device_id {
                    entry.is_mounted = false;
                }
            }
        }

        Ok(())
    }

    pub fn poll(&mut self) -> Result<(), DeviceManagerError> {
        while let Some(event) = self.source.poll_event()? {
            self.process_event(&event)?;
        }
        Ok(())
    }

    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    pub fn get_devices_json(&self) -> serde_json::Value {
        let devices: Vec<serde_json::Value> = self.devices.iter().map(|d| {
            serde_json::json!({
                "bus": format!("{:?}", d.bus),
                "vendor_id": format!("{:04x}", d.vendor_id),
                "device_id": format!("{:04x}", d.device_id),
                "class_code": format!("{:04x}", d.class_code),
                "description": d.description,
                "driver": d.driver,
                "mount_point": d.mount_point.as_ref().map(|p| p.to_string_lossy().to_string()),
            })
        }).collect();

        let mounts: Vec<serde_json::Value> = self.mounts.iter().map(|m| {
            serde_json::json!({
                "label": m.label,
                "mount_path": m.mount_path.to_string_lossy().to_string(),
                "is_mounted": m.is_mounted,
            })
        }).collect();

        serde_json::json!({
            "devices": devices,
            "mounts": mounts,
        })
    }

    pub fn list_device_infos(&self) -> Vec<DeviceInfo> {
        self.devices.clone()
    }
}

impl std::fmt::Debug for DeviceManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceManager")
            .field("device_count", &self.devices.len())
            .field("mount_count", &self.mounts.len())
            .field("rule_count", &self.driver_rules.rules.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Driver probe simulation (for testing)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverAction {
    LoadDriver { driver: String, vendor_id: u16, device_id: u16 },
    MountStorage { label: String, path: PathBuf },
    UnmountStorage { label: String, path: PathBuf },
    Ignore,
}

pub fn evaluate_driver_rule(
    event: &RawHotplugEvent,
    rules: &DriverRuleTable,
) -> DriverAction {
    let rule = rules.match_device(event.vendor_id, event.device_id);
    match rule {
        Some(r) if r.auto_probe => {
            let is_usb_storage = event.bus_type == DeviceBus::Usb && r.driver_name == "usb-storage";
            if is_usb_storage {
                let label = event.label.clone().unwrap_or_else(|| {
                    format!("usb-{:04x}-{:04x}", event.vendor_id, event.device_id)
                });
                let path = PathBuf::from(format!("/media/{label}"));
                DriverAction::MountStorage {
                    label,
                    path,
                }
            } else {
                DriverAction::LoadDriver {
                    driver: r.driver_name.clone(),
                    vendor_id: event.vendor_id,
                    device_id: event.device_id,
                }
            }
        }
        Some(_) => DriverAction::Ignore,
        None => DriverAction::Ignore,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── 46.2: Hotplug event parsing ────────────────────────────────────────

    #[test]
    fn test_parse_device_add_event() {
        let raw = serialize_hotplug_event(&RawHotplugEvent {
            event_type: HotplugEventType::DeviceAdded,
            vendor_id: 0x8086,
            device_id: 0x100e,
            class_code: 0x0200,
            subclass_code: 0,
            prog_if: 0,
            bus_number: 0,
            device_number: 1,
            function_number: 0,
            bus_type: DeviceBus::Pci,
            label: None,
        });

        let parsed = parse_hotplug_event(&raw).unwrap();
        assert_eq!(parsed.event_type, HotplugEventType::DeviceAdded);
        assert_eq!(parsed.vendor_id, 0x8086);
        assert_eq!(parsed.device_id, 0x100e);
        assert_eq!(parsed.bus_type, DeviceBus::Pci);
    }

    #[test]
    fn test_parse_device_remove_event() {
        let raw = serialize_hotplug_event(&RawHotplugEvent {
            event_type: HotplugEventType::DeviceRemoved,
            vendor_id: 0x1af4,
            device_id: 0x1000,
            class_code: 0x0200,
            subclass_code: 0,
            prog_if: 0,
            bus_number: 0,
            device_number: 2,
            function_number: 0,
            bus_type: DeviceBus::Pci,
            label: None,
        });

        let parsed = parse_hotplug_event(&raw).unwrap();
        assert_eq!(parsed.event_type, HotplugEventType::DeviceRemoved);
        assert_eq!(parsed.vendor_id, 0x1af4);
        assert_eq!(parsed.device_id, 0x1000);
    }

    #[test]
    fn test_parse_usb_storage_add_event() {
        let raw = serialize_hotplug_event(&RawHotplugEvent {
            event_type: HotplugEventType::DeviceAdded,
            vendor_id: 0x090c,
            device_id: 0x1000,
            class_code: 0x0800,
            subclass_code: 6,
            prog_if: 0x50,
            bus_number: 1,
            device_number: 0,
            function_number: 0,
            bus_type: DeviceBus::Usb,
            label: Some("SAMSUNG".into()),
        });

        let parsed = parse_hotplug_event(&raw).unwrap();
        assert_eq!(parsed.event_type, HotplugEventType::DeviceAdded);
        assert_eq!(parsed.vendor_id, 0x090c);
        assert_eq!(parsed.device_id, 0x1000);
        assert_eq!(parsed.bus_type, DeviceBus::Usb);
        assert_eq!(parsed.label.as_deref(), Some("SAMSUNG"));
    }

    #[test]
    fn test_parse_invalid_short_event() {
        let result = parse_hotplug_event(&[0x01, 0x02]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too short"));
    }

    #[test]
    fn test_parse_unknown_event_type() {
        let mut raw = vec![99u8; 13];
        raw[0] = 99;
        let result = parse_hotplug_event(&raw);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown event type"));
    }

    #[test]
    fn test_serialize_round_trip() {
        let event = RawHotplugEvent {
            event_type: HotplugEventType::DeviceAdded,
            vendor_id: 0x8086,
            device_id: 0x1237,
            class_code: 0x0601,
            subclass_code: 0,
            prog_if: 0,
            bus_number: 0,
            device_number: 0,
            function_number: 0,
            bus_type: DeviceBus::Pci,
            label: None,
        };

        let data = serialize_hotplug_event(&event);
        let parsed = parse_hotplug_event(&data).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn test_serialize_round_trip_with_label() {
        let event = RawHotplugEvent {
            event_type: HotplugEventType::DeviceAdded,
            vendor_id: 0x090c,
            device_id: 0x1000,
            class_code: 0x0800,
            subclass_code: 6,
            prog_if: 0x50,
            bus_number: 2,
            device_number: 0,
            function_number: 0,
            bus_type: DeviceBus::Usb,
            label: Some("SanDisk".into()),
        };

        let data = serialize_hotplug_event(&event);
        let parsed = parse_hotplug_event(&data).unwrap();
        assert_eq!(parsed, event);
    }

    // ── 46.2: Driver rule matching ─────────────────────────────────────────

    #[test]
    fn test_driver_rule_matches_known_device() {
        let rules = default_driver_rules();
        let rule = rules.match_device(0x8086, 0x100e);
        assert!(rule.is_some());
        assert_eq!(rule.unwrap().driver_name, "e1000");
    }

    #[test]
    fn test_driver_rule_no_match_unknown_device() {
        let rules = default_driver_rules();
        let rule = rules.match_device(0xdead, 0xbeef);
        assert!(rule.is_none());
    }

    #[test]
    fn test_driver_rule_matches_known_usb_storage() {
        let rules = default_driver_rules();
        let rule = rules.match_device(0x090c, 0x1000);
        assert!(rule.is_some());
        assert_eq!(rule.unwrap().driver_name, "usb-storage");
    }

    #[test]
    fn test_evaluate_driver_rule_known_device_triggers_load() {
        let event = RawHotplugEvent {
            event_type: HotplugEventType::DeviceAdded,
            vendor_id: 0x8086,
            device_id: 0x100e,
            class_code: 0x0200,
            subclass_code: 0,
            prog_if: 0,
            bus_number: 0,
            device_number: 1,
            function_number: 0,
            bus_type: DeviceBus::Pci,
            label: None,
        };

        let action = evaluate_driver_rule(&event, &default_driver_rules());
        assert_eq!(action, DriverAction::LoadDriver {
            driver: "e1000".into(),
            vendor_id: 0x8086,
            device_id: 0x100e,
        });
    }

    #[test]
    fn test_evaluate_driver_rule_usb_storage_triggers_mount() {
        let event = RawHotplugEvent {
            event_type: HotplugEventType::DeviceAdded,
            vendor_id: 0x090c,
            device_id: 0x1000,
            class_code: 0x0800,
            subclass_code: 6,
            prog_if: 0x50,
            bus_number: 1,
            device_number: 0,
            function_number: 0,
            bus_type: DeviceBus::Usb,
            label: Some("SAMSUNG".into()),
        };

        let action = evaluate_driver_rule(&event, &default_driver_rules());
        assert_eq!(action, DriverAction::MountStorage {
            label: "SAMSUNG".into(),
            path: PathBuf::from("/media/SAMSUNG"),
        });
    }

    #[test]
    fn test_evaluate_driver_rule_unknown_device_ignores() {
        let event = RawHotplugEvent {
            event_type: HotplugEventType::DeviceAdded,
            vendor_id: 0xdead,
            device_id: 0xbeef,
            class_code: 0x0000,
            subclass_code: 0,
            prog_if: 0,
            bus_number: 0,
            device_number: 0,
            function_number: 0,
            bus_type: DeviceBus::Pci,
            label: None,
        };

        let action = evaluate_driver_rule(&event, &default_driver_rules());
        assert_eq!(action, DriverAction::Ignore);
    }

    // ── Device manager integration ─────────────────────────────────────────

    fn make_net_device_add() -> RawHotplugEvent {
        RawHotplugEvent {
            event_type: HotplugEventType::DeviceAdded,
            vendor_id: 0x1af4,
            device_id: 0x1000,
            class_code: 0x0200,
            subclass_code: 0,
            prog_if: 0,
            bus_number: 0,
            device_number: 3,
            function_number: 0,
            bus_type: DeviceBus::Pci,
            label: None,
        }
    }

    fn make_usb_storage_add(label: &str) -> RawHotplugEvent {
        RawHotplugEvent {
            event_type: HotplugEventType::DeviceAdded,
            vendor_id: 0x090c,
            device_id: 0x1000,
            class_code: 0x0800,
            subclass_code: 6,
            prog_if: 0x50,
            bus_number: 2,
            device_number: 0,
            function_number: 0,
            bus_type: DeviceBus::Usb,
            label: Some(label.into()),
        }
    }

    fn make_device_remove(vendor_id: u16, device_id: u16, label: Option<&str>) -> RawHotplugEvent {
        RawHotplugEvent {
            event_type: HotplugEventType::DeviceRemoved,
            vendor_id,
            device_id,
            class_code: 0,
            subclass_code: 0,
            prog_if: 0,
            bus_number: 0,
            device_number: 3,
            function_number: 0,
            bus_type: DeviceBus::Pci,
            label: label.map(String::from),
        }
    }

    #[test]
    fn test_mock_hotplug_source_delivers_events() {
        let events = vec![make_net_device_add(), make_usb_storage_add("TESTDRIVE")];
        let mut source = MockHotplugSource::new(events);

        let e1 = source.poll_event().unwrap().unwrap();
        assert_eq!(e1.vendor_id, 0x1af4);
        assert_eq!(e1.device_id, 0x1000);

        let e2 = source.poll_event().unwrap().unwrap();
        assert_eq!(e2.vendor_id, 0x090c);
        assert_eq!(e2.device_id, 0x1000);
        assert_eq!(e2.label.as_deref(), Some("TESTDRIVE"));

        assert!(source.poll_event().unwrap().is_none());
    }

    #[test]
    fn test_device_manager_processes_added_device() {
        let source = MockHotplugSource::new(vec![make_net_device_add()]);
        let mut dm = DeviceManager::new(Box::new(source));

        dm.poll().unwrap();

        assert_eq!(dm.device_count(), 1);
        assert_eq!(dm.devices[0].vendor_id, 0x1af4);
        assert_eq!(dm.devices[0].device_id, 0x1000);
        assert_eq!(dm.devices[0].driver.as_deref(), Some("virtio-net"));
    }

    #[test]
    fn test_device_manager_adds_usb_storage_with_mount() {
        let source = MockHotplugSource::new(vec![make_usb_storage_add("SAMSUNG")]);
        let mut dm = DeviceManager::new(Box::new(source));

        dm.poll().unwrap();

        assert_eq!(dm.device_count(), 1);
        assert_eq!(dm.devices[0].mount_point.as_ref().map(|p| p.to_string_lossy().to_string()),
                   Some("/media/SAMSUNG".into()));

        assert_eq!(dm.mounts.len(), 1);
        assert!(dm.mounts[0].is_mounted);
        assert_eq!(dm.mounts[0].label, "SAMSUNG");
    }

    #[test]
    fn test_device_manager_removes_device() {
        let add = make_net_device_add();
        let remove = make_device_remove(0x1af4, 0x1000, None);
        let source = MockHotplugSource::new(vec![add, remove]);
        let mut dm = DeviceManager::new(Box::new(source));

        dm.poll().unwrap();

        assert_eq!(dm.device_count(), 0);
    }

    #[test]
    fn test_device_manager_usb_storage_unmounts_on_removal() {
        let add = make_usb_storage_add("SAMSUNG");
        let remove = RawHotplugEvent {
            event_type: HotplugEventType::DeviceRemoved,
            vendor_id: 0x090c,
            device_id: 0x1000,
            class_code: 0,
            subclass_code: 0,
            prog_if: 0,
            bus_number: 2,
            device_number: 0,
            function_number: 0,
            bus_type: DeviceBus::Usb,
            label: Some("SAMSUNG".into()),
        };
        let source = MockHotplugSource::new(vec![add, remove]);
        let mut dm = DeviceManager::new(Box::new(source));

        dm.poll().unwrap();

        assert_eq!(dm.device_count(), 0);
        assert!(!dm.mounts[0].is_mounted);
    }

    #[test]
    fn test_device_manager_multiple_devices() {
        let source = MockHotplugSource::new(vec![
            make_net_device_add(),
            RawHotplugEvent { vendor_id: 0x8086, device_id: 0x100e, ..make_net_device_add() },
            make_usb_storage_add("DRIVE1"),
        ]);
        let mut dm = DeviceManager::new(Box::new(source));

        dm.poll().unwrap();

        assert_eq!(dm.device_count(), 3);
        assert_eq!(dm.mounts.len(), 1);
        assert!(dm.mounts[0].is_mounted);
    }

    #[test]
    fn test_device_list_json_output() {
        let source = MockHotplugSource::new(vec![
            make_net_device_add(),
            make_usb_storage_add("DRIVE_A"),
        ]);
        let mut dm = DeviceManager::new(Box::new(source));
        dm.poll().unwrap();

        let json = dm.get_devices_json();
        let devices = json["devices"].as_array().unwrap();
        assert_eq!(devices.len(), 2);

        let mounts = json["mounts"].as_array().unwrap();
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0]["label"].as_str(), Some("DRIVE_A"));
    }

    #[test]
    fn test_evaluate_driver_rule_known_device_triggers_correct_driver() {
        // Core test for requirement 38.2: device-add with known vendor/device
        // ID triggers the correct driver rule.
        let test_cases = [
            (0x8086, 0x100e, "e1000"),
            (0x1af4, 0x1000, "virtio-net"),
            (0x1af4, 0x1001, "virtio-blk"),
            (0x090c, 0x1000, "usb-storage"),
        ];

        for &(vendor, device, expected_driver) in &test_cases {
            let event = RawHotplugEvent {
                event_type: HotplugEventType::DeviceAdded,
                vendor_id: vendor,
                device_id: device,
                class_code: 0,
                subclass_code: 0,
                prog_if: 0,
                bus_number: 0,
                device_number: 0,
                function_number: 0,
                bus_type: DeviceBus::Pci,
                label: None,
            };

            let action = evaluate_driver_rule(&event, &default_driver_rules());
            assert_eq!(
                action,
                DriverAction::LoadDriver {
                    driver: expected_driver.into(),
                    vendor_id: vendor,
                    device_id: device,
                },
                "Vendor {:04x} device {:04x} should match driver '{expected_driver}'",
                vendor, device
            );
        }
    }
}
