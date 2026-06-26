use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::net::UnixStream;

use service_manager::{ServiceManager, ServiceState, ServiceUnit};
#[cfg(unix)]
use turnix_ipc_proto::{
    ERROR_INTERNAL, ERROR_INVALID_ARGS, ERROR_SERVICE_NOT_FOUND, IpcError, IpcMessage, IpcValue,
    decode_message, encode_message,
};

// ---------------------------------------------------------------------------
// Helpers for IPC broker communication
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn send_msg(stream: &mut UnixStream, msg: &IpcMessage) {
    let bytes = encode_message(msg).unwrap_or_default();
    use std::io::Write;
    let _ = stream.write_all(&bytes);
}

#[cfg(unix)]
fn recv_msg(stream: &mut UnixStream) -> Option<IpcMessage> {
    use std::io::Read;
    let mut len_buf = [0u8; 4];
    if stream.read_exact(&mut len_buf).is_err() {
        return None;
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut payload = vec![0u8; len];
    if stream.read_exact(&mut payload).is_err() {
        return None;
    }
    let mut full = Vec::with_capacity(4 + len);
    full.extend_from_slice(&len_buf);
    full.extend_from_slice(&payload);
    decode_message(&full).ok().map(|(m, _)| m)
}

#[cfg(unix)]
fn ipc_value_string(s: &str) -> IpcValue {
    IpcValue::String(s.to_string())
}

#[cfg(unix)]
fn serialize_state(state: &ServiceState) -> IpcValue {
    match state {
        ServiceState::Stopped => ipc_value_string("stopped"),
        ServiceState::Starting => ipc_value_string("starting"),
        ServiceState::Running { pid, started_at } => IpcValue::Map(vec![
            ("status".into(), ipc_value_string("running")),
            ("pid".into(), IpcValue::Int(*pid as i64)),
            ("started_at".into(), IpcValue::Int(*started_at as i64)),
        ]),
        ServiceState::Failed {
            exit_code,
            retry_count,
            message,
        } => IpcValue::Map(vec![
            ("status".into(), ipc_value_string("failed")),
            (
                "exit_code".into(),
                IpcValue::Int(exit_code.unwrap_or(-1) as i64),
            ),
            ("retries".into(), IpcValue::Int(*retry_count as i64)),
            ("message".into(), ipc_value_string(message)),
        ]),
    }
}

// ---------------------------------------------------------------------------
// Tracked child process
// ---------------------------------------------------------------------------

#[cfg(unix)]
struct TrackedService {
    unit: ServiceUnit,
    child: Option<Child>,
    retry_count: u32,
    started_at: Instant,
}

#[cfg(unix)]
impl TrackedService {
    fn new(unit: ServiceUnit) -> Self {
        Self {
            unit,
            child: None,
            retry_count: 0,
            started_at: Instant::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Daemon entry point
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn main() {
    use std::os::unix::net::UnixStream;

    let units = load_service_units("/etc/turnix/services/");
    eprintln!("service-manager: loaded {} service units", units.len());

    let mut manager = ServiceManager::new();
    if let Err(e) = manager.load_units(units.clone()) {
        eprintln!("service-manager: dependency resolution failed: {e}");
        std::process::exit(1);
    }

    eprintln!("service-manager: start order: {:?}", manager.start_order());

    let mut tracked: HashMap<String, TrackedService> = HashMap::new();
    for unit in units {
        tracked.insert(unit.name.clone(), TrackedService::new(unit));
    }

    let mut broker = match UnixStream::connect("/run/ipc.sock") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("service-manager: cannot connect to ipc broker: {e}");
            std::process::exit(1);
        }
    };

    // Register with broker
    let register_msg = IpcMessage::MethodCall {
        id: 1,
        interface: "org.turnix.Broker".into(),
        method: "Register".into(),
        args: vec![ipc_value_string("org.turnix.ServiceManager")],
    };
    send_msg(&mut broker, &register_msg);

    if let Some(reply) = recv_msg(&mut broker) {
        match reply {
            IpcMessage::MethodReturn { result: Ok(_), .. } => {
                eprintln!("service-manager: registered with broker");
            }
            _ => {
                eprintln!("service-manager: registration failed");
                std::process::exit(1);
            }
        }
    }

    // Start services in order (clone the order to avoid borrow issues)
    let order: Vec<String> = manager.start_order().to_vec();
    for name in &order {
        start_service(&manager, &mut tracked, name);
    }

    // Main event loop
    broker
        .set_read_timeout(Some(Duration::from_millis(500)))
        .ok();
    let mut last_check = Instant::now();

    loop {
        if let Some(msg) = recv_msg(&mut broker) {
            handle_ipc_message(&manager, &mut tracked, &mut broker, msg);
            broker
                .set_read_timeout(Some(Duration::from_millis(500)))
                .ok();
        }

        if last_check.elapsed() >= Duration::from_millis(500) {
            check_services(&manager, &mut tracked);
            last_check = Instant::now();
        }
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("service-manager: requires Unix domain sockets; not supported on Windows");
    std::process::exit(1);
}

// ---------------------------------------------------------------------------
// Service loading
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn load_service_units(dir: &str) -> Vec<ServiceUnit> {
    let mut units = Vec::new();
    let dir_path = Path::new(dir);
    if !dir_path.is_dir() {
        eprintln!("service-manager: warning: {dir} does not exist, using empty config");
        return units;
    }

    if let Ok(entries) = std::fs::read_dir(dir_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(contents) => match ServiceUnit::from_toml(&contents) {
                    Ok(unit) => units.push(unit),
                    Err(e) => eprintln!("service-manager: skipping {:?}: {e}", path),
                },
                Err(e) => eprintln!("service-manager: cannot read {:?}: {e}", path),
            }
        }
    }
    units
}

// ---------------------------------------------------------------------------
// Service start
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn start_service(
    _manager: &ServiceManager,
    tracked: &mut HashMap<String, TrackedService>,
    name: &str,
) {
    // Clone the unit data we need before mutating tracked
    let (path, args, timeout, socket_spec) = match tracked.get(name) {
        Some(t) => (
            t.unit.path.clone(),
            t.unit.args.clone(),
            t.unit.timeout_start_sec,
            t.unit.socket.clone(),
        ),
        None => {
            eprintln!("service-manager: {name} not tracked");
            return;
        }
    };

    // Socket activation
    #[cfg(unix)]
    if let Some(ref spec) = socket_spec {
        match std::os::unix::net::UnixListener::bind(&spec.path) {
            Ok(listener) => {
                use std::os::fd::AsRawFd;
                let _fd = listener.as_raw_fd();
                let _ = listener;
                eprintln!("service-manager: pre-opened socket {}", spec.path);
            }
            Err(e) => {
                eprintln!("service-manager: cannot bind socket {}: {e}", spec.path);
            }
        }
    }
    #[cfg(not(unix))]
    if let Some(ref _spec) = socket_spec {
        eprintln!("service-manager: socket activation not supported on Windows");
    }

    eprintln!("service-manager: starting {name}...");

    let mut cmd = Command::new(&path);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    match cmd.spawn() {
        Ok(child) => {
            let pid = child.id() as u64;
            eprintln!("service-manager: {name} started (pid={pid})");

            if let Some(tracker) = tracked.get_mut(name) {
                tracker.child = Some(child);
                tracker.started_at = Instant::now();
                tracker.retry_count = 0;
            }

            // Wait for TimeoutStartSec then check if still alive
            std::thread::sleep(Duration::from_secs(timeout));

            let is_alive = tracked
                .get_mut(name)
                .and_then(|t| {
                    t.child
                        .as_mut()
                        .map(|c| !matches!(c.try_wait(), Ok(Some(_))))
                })
                .unwrap_or(false);

            if is_alive {
                eprintln!("service-manager: {name} started successfully within timeout");
            } else {
                eprintln!("service-manager: {name} failed within {timeout}s timeout");
            }
        }
        Err(e) => {
            eprintln!("service-manager: cannot spawn {name}: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Service health check
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn check_services(_manager: &ServiceManager, tracked: &mut HashMap<String, TrackedService>) {
    let names: Vec<String> = tracked.keys().cloned().collect();
    for name in names {
        let should_restart = {
            let tracker = match tracked.get_mut(&name) {
                Some(t) => t,
                None => continue,
            };
            let child = match tracker.child.as_mut() {
                Some(c) => c,
                None => continue,
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    let code = status.code().unwrap_or(-1);
                    eprintln!("service-manager: {name} exited with code {code}");
                    _manager.should_restart(&name, code)
                }
                _ => continue,
            }
        };

        if should_restart {
            let retry = tracked.get(&name).map(|t| t.retry_count).unwrap_or(0);
            let delay = ServiceManager::backoff_delay(retry);
            eprintln!("service-manager: restarting {name} in {delay}s (retry #{retry})");
            std::thread::sleep(Duration::from_secs(delay));

            if let Some(tracker) = tracked.get_mut(&name) {
                tracker.retry_count += 1;
            }
            start_service(_manager, tracked, &name);
        }
    }
}

// ---------------------------------------------------------------------------
// IPC message handler
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn handle_ipc_message(
    manager: &ServiceManager,
    tracked: &mut HashMap<String, TrackedService>,
    broker: &mut UnixStream,
    msg: IpcMessage,
) {
    if let IpcMessage::MethodCall {
        id, method, args, ..
    } = msg
    {
        let response = match method.as_str() {
            "Start" => {
                let svc_name = args.first().and_then(|v| v.as_str()).unwrap_or("");
                if svc_name.is_empty() {
                    IpcMessage::MethodReturn {
                        id,
                        result: Err(IpcError::new(ERROR_INVALID_ARGS, "missing service name")),
                    }
                } else if !tracked.contains_key(svc_name) {
                    IpcMessage::MethodReturn {
                        id,
                        result: Err(IpcError::new(
                            ERROR_SERVICE_NOT_FOUND,
                            format!("service {svc_name} not found"),
                        )),
                    }
                } else {
                    start_service(manager, tracked, svc_name);
                    IpcMessage::MethodReturn {
                        id,
                        result: Ok(ipc_value_string("started")),
                    }
                }
            }
            "Stop" => {
                let svc_name = args.first().and_then(|v| v.as_str()).unwrap_or("");
                if let Some(tracker) = tracked.get_mut(svc_name) {
                    if let Some(mut child) = tracker.child.take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    IpcMessage::MethodReturn {
                        id,
                        result: Ok(ipc_value_string("stopped")),
                    }
                } else {
                    IpcMessage::MethodReturn {
                        id,
                        result: Err(IpcError::new(
                            ERROR_SERVICE_NOT_FOUND,
                            format!("service {svc_name} not found"),
                        )),
                    }
                }
            }
            "Status" => {
                let svc_name = args.first().and_then(|v| v.as_str()).unwrap_or("");
                match manager.state(svc_name) {
                    Some(state) => IpcMessage::MethodReturn {
                        id,
                        result: Ok(serialize_state(state)),
                    },
                    None => IpcMessage::MethodReturn {
                        id,
                        result: Err(IpcError::new(
                            ERROR_SERVICE_NOT_FOUND,
                            format!("service {svc_name} not found"),
                        )),
                    },
                }
            }
            "List" => {
                let services: Vec<IpcValue> = manager
                    .service_names()
                    .into_iter()
                    .map(|nm| {
                        let state = manager.state(&nm);
                        let state_val = state
                            .map(serialize_state)
                            .unwrap_or(ipc_value_string("unknown"));
                        IpcValue::Map(vec![
                            ("name".into(), ipc_value_string(&nm)),
                            ("state".into(), state_val),
                        ])
                    })
                    .collect();
                IpcMessage::MethodReturn {
                    id,
                    result: Ok(IpcValue::Array(services)),
                }
            }
            _ => IpcMessage::MethodReturn {
                id,
                result: Err(IpcError::new(
                    ERROR_INTERNAL,
                    format!("unknown method {method}"),
                )),
            },
        };
        send_msg(broker, &response);
    }
}
