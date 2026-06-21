use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use device_manager::{
    default_driver_rules, DriverRuleTable, HotplugSource, RawHotplugEvent, DeviceBus,
    HotplugEventType, parse_hotplug_event,
};
use ipc_proto::{encode_message, decode_message, IpcMessage, IpcValue, IpcError};

// ---------------------------------------------------------------------------
// Real hotplug source (kernel hotplug_subscribe syscall)
// ---------------------------------------------------------------------------

pub struct KernelHotplugSource {
    file: Option<std::fs::File>,
}

impl Default for KernelHotplugSource {
    fn default() -> Self {
        Self::new()
    }
}

impl KernelHotplugSource {
    pub fn new() -> Self {
        let file = std::fs::File::open("/dev/hotplug").ok();
        Self { file }
    }
}

impl HotplugSource for KernelHotplugSource {
    fn poll_event(&mut self) -> Result<Option<RawHotplugEvent>, device_manager::DeviceManagerError> {
        let file = match self.file.as_mut() {
            Some(f) => f,
            None => return Ok(None),
        };

        let mut buf = [0u8; 256];
        match file.read(&mut buf) {
            Ok(0) => Ok(None),
            Ok(n) => {
                let event = parse_hotplug_event(&buf[..n])?;
                Ok(Some(event))
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(device_manager::DeviceManagerError::HotplugError(format!("read error: {e}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// CLI helpers
// ---------------------------------------------------------------------------

struct CliConfig {
    rules_path: String,
}

fn parse_args() -> CliConfig {
    let args: Vec<String> = std::env::args().collect();
    let mut rules_path = "/etc/turnix/driver-rules.toml".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--rules" | "-r" => {
                i += 1;
                if i < args.len() {
                    rules_path = args[i].clone();
                }
            }
            "--help" | "-h" => {
                println!("Usage: {} [OPTIONS]", args[0]);
                println!("  --rules, -r PATH   Driver rules TOML file");
                println!("  --help, -h         Show this help");
                std::process::exit(0);
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                std::process::exit(1);
            }
        }
        i += 1;
    }

    CliConfig { rules_path }
}

// ---------------------------------------------------------------------------
// IPC helpers
// ---------------------------------------------------------------------------

fn send_msg(stream: &mut UnixStream, msg: &IpcMessage) {
    if let Ok(encoded) = encode_message(msg) {
        let _ = stream.write_all(&encoded);
    }
}

fn recv_msg(stream: &mut UnixStream) -> Option<IpcMessage> {
    let mut len_buf = [0u8; 4];
    if stream.read_exact(&mut len_buf).is_err() { return None; }
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut payload = vec![0u8; len];
    if stream.read_exact(&mut payload).is_err() { return None; }
    let mut full = Vec::with_capacity(4 + len);
    full.extend_from_slice(&len_buf);
    full.extend_from_slice(&payload);
    decode_message(&full).ok().map(|(m, _)| m)
}

fn call_method(
    stream: &mut UnixStream,
    interface: &str,
    method: &str,
    args: Vec<IpcValue>,
) -> Option<IpcValue> {
    let call = IpcMessage::MethodCall {
        id: 1,
        interface: interface.into(),
        method: method.into(),
        args,
    };
    send_msg(stream, &call);

    loop {
        match recv_msg(stream)? {
            IpcMessage::MethodReturn { id: _, result } => {
                return result.ok();
            }
            _ => continue,
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let config = parse_args();

    // Load driver rules from TOML (fall back to defaults)
    let rules = match DriverRuleTable::load_from_toml(&config.rules_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Warning: cannot load rules from '{0}': {e}", config.rules_path);
            eprintln!("Using default driver rules");
            default_driver_rules()
        }
    };

    // Create hotplug source (kernel device or mock if not available)
    let source: Box<dyn HotplugSource> = {
        let kernel_source = KernelHotplugSource::new();
        if kernel_source.file.is_some() {
            Box::new(kernel_source)
        } else {
            eprintln!("Warning: /dev/hotplug not available, running in mock mode");
            let events = mock_boot_devices();
            Box::new(device_manager::MockHotplugSource::new(events))
        }
    };

    let mut dm = device_manager::DeviceManager::with_rules(source, rules);

    // Connect to IPC broker
    let mut stream = match UnixStream::connect("/run/ipc.sock") {
        Ok(s) => {
            s.set_read_timeout(Some(Duration::from_millis(500))).ok();
            s
        }
        Err(e) => {
            eprintln!("Error: cannot connect to IPC broker: {e}");
            std::process::exit(1);
        }
    };

    // Register with broker
    match call_method(
        &mut stream,
        "org.turnix.Broker",
        "Register",
        vec![IpcValue::String("org.turnix.DeviceManager".into())],
    ) {
        Some(IpcValue::String(s)) => {
            dm.registered_name = Some(s);
            eprintln!("Registered as org.turnix.DeviceManager");
        }
        _ => {
            eprintln!("Registered with broker");
        }
    }

    eprintln!("Device manager started");

    // Event loop
    loop {
        // Poll hotplug events
        if let Err(e) = dm.poll() {
            eprintln!("Hotplug poll error: {e}");
        }

        // Poll IPC broker for method calls
        if let Some(msg) = recv_msg(&mut stream) {
            handle_ipc(&mut stream, &mut dm, msg);
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

fn handle_ipc(
    stream: &mut UnixStream,
    dm: &mut device_manager::DeviceManager,
    msg: IpcMessage,
) {
    let resp = match msg {
        IpcMessage::MethodCall { id, method, .. } => {
            let result = match method.as_str() {
                "ListDevices" => {
                    let infos = dm.list_device_infos();
                    let devices: Vec<IpcValue> = infos.into_iter().map(|d| {
                        IpcValue::Map(vec![
                            ("vendor_id".into(), IpcValue::String(format!("{:04x}", d.vendor_id))),
                            ("device_id".into(), IpcValue::String(format!("{:04x}", d.device_id))),
                            ("description".into(), IpcValue::String(d.description)),
                            ("driver".into(), IpcValue::String(d.driver.unwrap_or_else(|| "none".into()))),
                            ("bus".into(), IpcValue::String(format!("{:?}", d.bus))),
                        ])
                    }).collect();

                    let mounts: Vec<IpcValue> = dm.mounts.iter().map(|m| {
                        IpcValue::Map(vec![
                            ("label".into(), IpcValue::String(m.label.clone())),
                            ("mount_path".into(), IpcValue::String(m.mount_path.to_string_lossy().to_string())),
                            ("is_mounted".into(), IpcValue::Bool(m.is_mounted)),
                        ])
                    }).collect();

                    Ok(IpcValue::Map(vec![
                        ("devices".into(), IpcValue::Array(devices)),
                        ("mounts".into(), IpcValue::Array(mounts)),
                    ]))
                }
                "DeviceCount" => {
                    Ok(IpcValue::Int(dm.device_count() as i64))
                }
                _ => {
                    Err(IpcError {
                        code: ipc_proto::ERROR_METHOD_NOT_FOUND,
                        message: format!("unknown method: {method}"),
                    })
                }
            };

            Some(IpcMessage::MethodReturn { id, result })
        }
        _ => None,
    };

    if let Some(r) = resp {
        send_msg(stream, &r);
    }
}

fn mock_boot_devices() -> Vec<RawHotplugEvent> {
    vec![
        RawHotplugEvent {
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
        },
        RawHotplugEvent {
            event_type: HotplugEventType::DeviceAdded,
            vendor_id: 0x8086,
            device_id: 0x7000,
            class_code: 0x0101,
            subclass_code: 0,
            prog_if: 0,
            bus_number: 0,
            device_number: 1,
            function_number: 0,
            bus_type: DeviceBus::Pci,
            label: None,
        },
        RawHotplugEvent {
            event_type: HotplugEventType::DeviceAdded,
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
        },
    ]
}
