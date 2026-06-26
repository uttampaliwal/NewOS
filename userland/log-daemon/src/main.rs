use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

#[cfg(unix)]
use log_daemon::{FileKernelLogSource, KernelLogSource, LogEntry, LogRotator};
#[cfg(not(unix))]
use log_daemon::{LogEntry, LogRotator};
#[cfg(unix)]
use turnix_ipc_proto::{
    ERROR_INTERNAL, ERROR_INVALID_ARGS, IpcError, IpcMessage, IpcValue, decode_message,
    encode_message,
};

// ---------------------------------------------------------------------------
// Shorthand helpers
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn ipc_str(s: &str) -> IpcValue {
    IpcValue::String(s.to_string())
}

#[cfg(unix)]
fn send_msg(stream: &mut UnixStream, msg: &IpcMessage) {
    if let Ok(bytes) = encode_message(msg) {
        let _ = stream.write_all(&bytes);
    }
}

#[cfg(unix)]
fn recv_msg(stream: &mut UnixStream) -> Option<IpcMessage> {
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

// ---------------------------------------------------------------------------
// HMAC key — in production the kernel provides this at boot
// ---------------------------------------------------------------------------

#[cfg(unix)]
const HMAC_KEY: &[u8] = b"turnix-kernel-log-key-2026";

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

#[cfg(unix)]
struct LogDaemon {
    rotator: Mutex<LogRotator>,
}

#[cfg(unix)]
impl LogDaemon {
    fn open(log_base: &Path) -> Result<Self, String> {
        let rotator = LogRotator::open(log_base)?;
        Ok(Self {
            rotator: Mutex::new(rotator),
        })
    }

    /// Accept a log entry from a client, seal it, and write to the log file.
    fn submit_entry(&self, entry: &mut LogEntry) -> Result<(), String> {
        entry.seal(HMAC_KEY);
        let json = entry.to_json()?;
        let mut line = json;
        line.push(b'\n');
        let mut rotator = self.rotator.lock().unwrap();
        rotator.write(&line)
    }

    /// Query recent log entries by reading the current log file.
    fn recent_entries(&self, count: usize) -> Result<Vec<LogEntry>, String> {
        let _rotator = self.rotator.lock().unwrap();
        // Read the current log file
        let log_path = Path::new("/var/log/turnix.log");
        let content = fs::read_to_string(log_path).map_err(|e| format!("cannot read log: {e}"))?;
        let mut entries: Vec<LogEntry> = Vec::new();
        for line in content.lines().rev() {
            if line.is_empty() {
                continue;
            }
            if let Ok(entry) = LogEntry::from_json(line.as_bytes()) {
                entries.push(entry);
                if entries.len() >= count {
                    break;
                }
            }
        }
        entries.reverse();
        Ok(entries)
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn main() {
    use std::os::unix::net::{UnixListener, UnixStream};

    // Ensure /var/log exists
    let _ = fs::create_dir_all("/var/log");

    let daemon = match LogDaemon::open(Path::new("/var/log/turnix")) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("log-daemon: failed to open log: {e}");
            std::process::exit(1);
        }
    };

    // Remove stale socket
    let _ = fs::remove_file("/run/log.sock");
    let listener = match UnixListener::bind("/run/log.sock") {
        Ok(l) => l,
        Err(e) => {
            eprintln!("log-daemon: cannot bind /run/log.sock: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("log-daemon: listening on /run/log.sock");

    // Connect to IPC broker
    let mut broker = match UnixStream::connect("/run/ipc.sock") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("log-daemon: cannot connect to broker: {e}");
            std::process::exit(1);
        }
    };

    let register = IpcMessage::MethodCall {
        id: 1,
        interface: "org.turnix.Broker".into(),
        method: "Register".into(),
        args: vec![ipc_str("org.turnix.LogDaemon")],
    };
    send_msg(&mut broker, &register);
    if let Some(reply) = recv_msg(&mut broker) {
        match reply {
            IpcMessage::MethodReturn { result: Ok(_), .. } => {
                eprintln!("log-daemon: registered with broker");
            }
            _ => {
                eprintln!("log-daemon: broker registration failed");
                std::process::exit(1);
            }
        }
    }

    // Set up kernel log forwarder: read from /var/log/kernel.log if available
    let kernel_log_path = "/var/log/kernel.log";
    if Path::new(kernel_log_path).exists() {
        eprintln!("log-daemon: forwarding kernel logs from {kernel_log_path}");
    } else {
        eprintln!("log-daemon: kernel log forwarding disabled ({kernel_log_path} not found)");
    }
    let mut kernel_log: Box<dyn KernelLogSource> =
        Box::new(FileKernelLogSource::new(kernel_log_path));

    // Main event loop: accept log submissions on the socket
    broker
        .set_read_timeout(Some(Duration::from_millis(500)))
        .ok();
    listener.set_nonblocking(true).ok();

    loop {
        // Accept new log-client connections
        while let Some(mut stream) = accept_one(&listener) {
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                match stream.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
            // Parse each JSON line
            for line in buf.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
                match LogEntry::from_json(line) {
                    Ok(mut entry) => {
                        if let Err(e) = daemon.submit_entry(&mut entry) {
                            eprintln!("log-daemon: write error: {e}");
                        }
                    }
                    Err(e) => {
                        eprintln!("log-daemon: parse error: {e}");
                    }
                }
            }
        }

        // Poll kernel log
        if let Some(line) = kernel_log.poll() {
            let mut entry = LogEntry::new(line.level, "kernel", &line.message);
            if let Err(e) = daemon.submit_entry(&mut entry) {
                eprintln!("log-daemon: kernel log write error: {e}");
            }
        }

        // Handle IPC requests
        if let Some(msg) = recv_msg(&mut broker) {
            handle_ipc(&daemon, &mut broker, msg);
            broker
                .set_read_timeout(Some(Duration::from_millis(500)))
                .ok();
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("log-daemon: requires Unix domain sockets; not supported on Windows");
    std::process::exit(1);
}

#[cfg(unix)]
fn accept_one(listener: &UnixListener) -> Option<UnixStream> {
    match listener.accept() {
        Ok((stream, _)) => {
            stream.set_nonblocking(true).ok();
            Some(stream)
        }
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => None,
        Err(_) => None,
    }
}

// ---------------------------------------------------------------------------
// IPC handlers
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn handle_ipc(daemon: &LogDaemon, broker: &mut UnixStream, msg: IpcMessage) {
    if let IpcMessage::MethodCall {
        id, method, args, ..
    } = msg
    {
        let response = match method.as_str() {
            "Submit" => {
                let json_str = args.first().and_then(|v| v.as_str()).unwrap_or("");
                match LogEntry::from_json(json_str.as_bytes()) {
                    Ok(mut entry) => match daemon.submit_entry(&mut entry) {
                        Ok(()) => IpcMessage::MethodReturn {
                            id,
                            result: Ok(ipc_str("logged")),
                        },
                        Err(e) => IpcMessage::MethodReturn {
                            id,
                            result: Err(IpcError::new(ERROR_INTERNAL, e)),
                        },
                    },
                    Err(e) => IpcMessage::MethodReturn {
                        id,
                        result: Err(IpcError::new(ERROR_INVALID_ARGS, e)),
                    },
                }
            }
            "Recent" => {
                let n = args
                    .first()
                    .and_then(|v| v.as_i64())
                    .unwrap_or(10)
                    .clamp(1, 1000) as usize;
                match daemon.recent_entries(n) {
                    Ok(entries) => {
                        let arr: Vec<IpcValue> = entries
                            .into_iter()
                            .map(|e| {
                                let json = serde_json::to_value(&e).unwrap_or_default();
                                ipc_value_from_json(json)
                            })
                            .collect();
                        IpcMessage::MethodReturn {
                            id,
                            result: Ok(IpcValue::Array(arr)),
                        }
                    }
                    Err(e) => IpcMessage::MethodReturn {
                        id,
                        result: Err(IpcError::new(ERROR_INTERNAL, e)),
                    },
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

#[cfg(unix)]
fn ipc_value_from_json(val: serde_json::Value) -> IpcValue {
    match val {
        serde_json::Value::Null => IpcValue::Null,
        serde_json::Value::Bool(b) => IpcValue::Bool(b),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(IpcValue::Int)
            .or_else(|| n.as_f64().map(IpcValue::Float))
            .unwrap_or(IpcValue::Null),
        serde_json::Value::String(s) => IpcValue::String(s),
        serde_json::Value::Array(arr) => {
            IpcValue::Array(arr.into_iter().map(ipc_value_from_json).collect())
        }
        serde_json::Value::Object(map) => {
            let pairs: Vec<(String, IpcValue)> = map
                .into_iter()
                .map(|(k, v)| (k, ipc_value_from_json(v)))
                .collect();
            IpcValue::Map(pairs)
        }
    }
}
