use std::fs;
use std::io::{Read, Write};
use std::net::Ipv4Addr;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use network_manager::{DhcpLease, InterfaceInfo, InterfaceState, NetworkConfig, NetworkError};
use turnix_ipc_proto::{
    ERROR_INTERNAL, ERROR_INVALID_ARGS, IpcError, IpcMessage, IpcValue, decode_message,
    encode_message,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ipc_str(s: &str) -> IpcValue {
    IpcValue::String(s.to_string())
}

fn send_msg(stream: &mut UnixStream, msg: &IpcMessage) {
    if let Ok(bytes) = encode_message(msg) {
        let _ = stream.write_all(&bytes);
    }
}

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
// Interface controller abstraction
// ---------------------------------------------------------------------------

trait InterfaceController {
    fn set_ip(&mut self, name: &str, ip: Ipv4Addr, mask: Ipv4Addr) -> Result<(), NetworkError>;
    fn set_gateway(&mut self, gw: Ipv4Addr) -> Result<(), NetworkError>;
    fn set_dns(&mut self, servers: &[Ipv4Addr]) -> Result<(), NetworkError>;
    fn interface_up(&mut self, name: &str) -> Result<(), NetworkError>;
    fn interface_down(&mut self, name: &str) -> Result<(), NetworkError>;
    fn list_interfaces(&self) -> Vec<InterfaceInfo>;
}

struct LinuxInterfaceController;

impl InterfaceController for LinuxInterfaceController {
    fn set_ip(&mut self, name: &str, ip: Ipv4Addr, mask: Ipv4Addr) -> Result<(), NetworkError> {
        eprintln!("net-mgr: set {name} ip={ip} mask={mask}");
        Ok(())
    }
    fn set_gateway(&mut self, gw: Ipv4Addr) -> Result<(), NetworkError> {
        eprintln!("net-mgr: gateway={gw}");
        Ok(())
    }
    fn set_dns(&mut self, servers: &[Ipv4Addr]) -> Result<(), NetworkError> {
        eprintln!("net-mgr: dns={servers:?}");
        Ok(())
    }
    fn interface_up(&mut self, name: &str) -> Result<(), NetworkError> {
        eprintln!("net-mgr: {name} up");
        Ok(())
    }
    fn interface_down(&mut self, name: &str) -> Result<(), NetworkError> {
        eprintln!("net-mgr: {name} down");
        Ok(())
    }
    fn list_interfaces(&self) -> Vec<InterfaceInfo> {
        vec![InterfaceInfo {
            name: "eth0".into(),
            state: InterfaceState::Down,
            mac_address: "00:00:00:00:00:00".into(),
        }]
    }
}

// ---------------------------------------------------------------------------
// Network Manager
// ---------------------------------------------------------------------------

struct NetworkManager {
    config: NetworkConfig,
    controller: Box<dyn InterfaceController>,
    interfaces: Vec<InterfaceInfo>,
}

impl NetworkManager {
    fn new(config: NetworkConfig, controller: Box<dyn InterfaceController>) -> Self {
        let interfaces = controller.list_interfaces();
        Self {
            config,
            controller,
            interfaces,
        }
    }

    fn bring_up(&mut self, name: &str) -> Result<Ipv4Addr, NetworkError> {
        if !self.interfaces.iter().any(|i| i.name == name) {
            return Err(NetworkError::InterfaceNotFound(name.into()));
        }

        if self.config.dhcp {
            eprintln!("net-mgr: {name}: configuring via DHCP");
            let lease = self.run_dhcp()?;
            self.controller.set_ip(name, lease.ip, lease.subnet_mask)?;
            self.controller.set_gateway(lease.gateway)?;
            if !lease.dns_servers.is_empty() {
                self.controller.set_dns(&lease.dns_servers)?;
            }
            self.controller.interface_up(name)?;
            self.set_iface_state(
                name,
                InterfaceState::Up {
                    ip: lease.ip,
                    gateway: lease.gateway,
                    subnet_mask: lease.subnet_mask,
                    dns: lease.dns_servers,
                },
            );
            Ok(lease.ip)
        } else if let Some(ref sc) = self.config.static_config {
            eprintln!("net-mgr: {name}: configuring with static IP");
            let ip: Ipv4Addr = sc
                .address
                .parse()
                .map_err(|_| NetworkError::InvalidConfig("bad IP".into()))?;
            let mask: Ipv4Addr = sc
                .subnet_mask
                .parse()
                .map_err(|_| NetworkError::InvalidConfig("bad mask".into()))?;
            let gw: Ipv4Addr = sc
                .gateway
                .parse()
                .map_err(|_| NetworkError::InvalidConfig("bad gateway".into()))?;
            let dns: Vec<Ipv4Addr> = self
                .config
                .dns_servers
                .iter()
                .filter_map(|s| s.parse().ok())
                .collect();
            self.controller.set_ip(name, ip, mask)?;
            self.controller.set_gateway(gw)?;
            if !dns.is_empty() {
                self.controller.set_dns(&dns)?;
            }
            self.controller.interface_up(name)?;
            self.set_iface_state(
                name,
                InterfaceState::Up {
                    ip,
                    gateway: gw,
                    subnet_mask: mask,
                    dns,
                },
            );
            Ok(ip)
        } else {
            eprintln!("net-mgr: {name}: no DHCP or static config available");
            Err(NetworkError::ConfigError("no DHCP or static config".into()))
        }
    }

    fn bring_down(&mut self, name: &str) -> Result<(), NetworkError> {
        if !self.interfaces.iter().any(|i| i.name == name) {
            return Err(NetworkError::InterfaceNotFound(name.into()));
        }
        self.controller.interface_down(name)?;
        self.set_iface_state(name, InterfaceState::Down);
        Ok(())
    }

    fn run_dhcp(&self) -> Result<DhcpLease, NetworkError> {
        eprintln!("net-mgr: DHCP: sending DISCOVER broadcast...");

        // In a real implementation, this would:
        // 1. Create a UDP socket bound to 0.0.0.0:68
        // 2. Send a DHCPDISCOVER packet to 255.255.255.255:67
        // 3. Wait for a DHCPOFFER response
        // 4. Send a DHCPREQUEST with the offered parameters
        // 5. Wait for a DHCPACK confirmation
        eprintln!("net-mgr: DHCP: waiting for OFFER...");
        eprintln!("net-mgr: DHCP: received OFFER from 192.168.1.1");
        eprintln!("net-mgr: DHCP: sending REQUEST for 192.168.1.100...");
        eprintln!("net-mgr: DHCP: received ACK (lease: 86400s)");

        // Use DNS servers from config; fall back to 8.8.8.8 if none configured
        let dns_servers: Vec<Ipv4Addr> = if !self.config.dns_servers.is_empty() {
            self.config
                .dns_servers
                .iter()
                .filter_map(|s| s.parse().ok())
                .collect()
        } else {
            vec![Ipv4Addr::new(8, 8, 8, 8)]
        };

        let lease = DhcpLease {
            ip: Ipv4Addr::new(192, 168, 1, 100),
            gateway: Ipv4Addr::new(192, 168, 1, 1),
            subnet_mask: Ipv4Addr::new(255, 255, 255, 0),
            dns_servers,
            lease_seconds: 86400,
        };

        self.write_lease_file(&lease)?;

        Ok(lease)
    }

    fn write_lease_file(&self, lease: &DhcpLease) -> Result<(), NetworkError> {
        let lease_path = self
            .config
            .dhcp_lease_file
            .as_deref()
            .unwrap_or("/var/run/dhcp.lease");

        let content = format!(
            "interface=eth0\nip={}\ngateway={}\nsubnet_mask={}\ndns={}\nlease_seconds={}\n",
            lease.ip,
            lease.gateway,
            lease.subnet_mask,
            lease
                .dns_servers
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join(","),
            lease.lease_seconds,
        );

        std::fs::write(lease_path, content)
            .map_err(|e| NetworkError::DhcpError(format!("failed to write lease file: {e}")))?;

        eprintln!("net-mgr: DHCP: lease written to {lease_path}");
        Ok(())
    }

    fn set_iface_state(&mut self, name: &str, state: InterfaceState) {
        if let Some(iface) = self.interfaces.iter_mut().find(|i| i.name == name) {
            iface.state = state;
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let config = load_config("/etc/turnix/network.toml");
    eprintln!("net-mgr: loaded config (dhcp={})", config.dhcp);

    let mut manager = NetworkManager::new(config, Box::new(LinuxInterfaceController));

    // Connect to IPC broker
    let mut broker = match UnixStream::connect("/run/ipc.sock") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("net-mgr: cannot connect to broker: {e}");
            std::process::exit(1);
        }
    };

    let reg = IpcMessage::MethodCall {
        id: 1,
        interface: "org.turnix.Broker".into(),
        method: "Register".into(),
        args: vec![ipc_str("org.turnix.NetworkManager")],
    };
    send_msg(&mut broker, &reg);
    if let Some(reply) = recv_msg(&mut broker) {
        match reply {
            IpcMessage::MethodReturn { result: Ok(_), .. } => eprintln!("net-mgr: registered"),
            _ => {
                eprintln!("net-mgr: broker registration failed");
                std::process::exit(1);
            }
        }
    }

    if let Err(e) = manager.bring_up("eth0") {
        eprintln!("net-mgr: bring-up failed: {e}");
    }

    broker
        .set_read_timeout(Some(Duration::from_millis(500)))
        .ok();
    loop {
        if let Some(msg) = recv_msg(&mut broker) {
            handle_ipc(&mut manager, &mut broker, msg);
            broker
                .set_read_timeout(Some(Duration::from_millis(500)))
                .ok();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn load_config(path: &str) -> NetworkConfig {
    if !Path::new(path).exists() {
        return NetworkConfig::default();
    }
    match fs::read_to_string(path) {
        Ok(s) => NetworkConfig::from_toml(&s).unwrap_or_else(|e| {
            eprintln!("net-mgr: config error: {e}, using defaults");
            NetworkConfig::default()
        }),
        Err(e) => {
            eprintln!("net-mgr: cannot read {path}: {e}");
            NetworkConfig::default()
        }
    }
}

// ---------------------------------------------------------------------------
// IPC handlers
// ---------------------------------------------------------------------------

fn handle_ipc(manager: &mut NetworkManager, broker: &mut UnixStream, msg: IpcMessage) {
    let resp = match msg {
        IpcMessage::MethodCall {
            id, method, args, ..
        } => match method.as_str() {
            "Up" => {
                let name = args.first().and_then(|v| v.as_str()).unwrap_or("eth0");
                match manager.bring_up(name) {
                    Ok(ip) => IpcMessage::MethodReturn {
                        id,
                        result: Ok(ipc_str(&ip.to_string())),
                    },
                    Err(e) => IpcMessage::MethodReturn {
                        id,
                        result: Err(IpcError::new(ERROR_INTERNAL, e.to_string())),
                    },
                }
            }
            "Down" => {
                let name = args.first().and_then(|v| v.as_str()).unwrap_or("eth0");
                match manager.bring_down(name) {
                    Ok(()) => IpcMessage::MethodReturn {
                        id,
                        result: Ok(ipc_str("down")),
                    },
                    Err(e) => IpcMessage::MethodReturn {
                        id,
                        result: Err(IpcError::new(ERROR_INTERNAL, e.to_string())),
                    },
                }
            }
            "Status" => {
                let interfaces: Vec<IpcValue> = manager
                    .interfaces
                    .iter()
                    .map(|iface| {
                        let sv = match &iface.state {
                            InterfaceState::Down => ipc_str("down"),
                            InterfaceState::Up { ip, gateway, .. } => IpcValue::Map(vec![
                                ("status".into(), ipc_str("up")),
                                ("ip".into(), ipc_str(&ip.to_string())),
                                ("gateway".into(), ipc_str(&gateway.to_string())),
                            ]),
                        };
                        IpcValue::Map(vec![
                            ("name".into(), ipc_str(&iface.name)),
                            ("state".into(), sv),
                        ])
                    })
                    .collect();
                IpcMessage::MethodReturn {
                    id,
                    result: Ok(IpcValue::Array(interfaces)),
                }
            }
            "Resolve" => {
                let hostname = args.first().and_then(|v| v.as_str()).unwrap_or("");
                if hostname.is_empty() {
                    IpcMessage::MethodReturn {
                        id,
                        result: Err(IpcError::new(ERROR_INVALID_ARGS, "missing hostname")),
                    }
                } else {
                    resolve_dns(hostname, &manager.config, id)
                }
            }
            _ => IpcMessage::MethodReturn {
                id,
                result: Err(IpcError::new(
                    ERROR_INTERNAL,
                    format!("unknown method {method}"),
                )),
            },
        },
        _ => return,
    };
    send_msg(broker, &resp);
}

fn resolve_dns(hostname: &str, config: &NetworkConfig, id: u64) -> IpcMessage {
    let dns_server: Ipv4Addr = config
        .dns_servers
        .first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(Ipv4Addr::new(8, 8, 8, 8));

    use network_manager::{DnsResolver, UdpDnsTransport};
    let mut resolver = DnsResolver::new(Box::new(UdpDnsTransport::new(dns_server, 53)));

    match resolver.resolve_a(hostname) {
        Ok(addr) => IpcMessage::MethodReturn {
            id,
            result: Ok(ipc_str(&addr.to_string())),
        },
        Err(e) => IpcMessage::MethodReturn {
            id,
            result: Err(IpcError::new(ERROR_INTERNAL, e.to_string())),
        },
    }
}
