use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};
use smoltcp::wire::{DhcpPacket, DhcpRepr, Ipv4Address, DnsPacket};

pub mod dhcp;
pub mod netconfig;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkError {
    ConfigError(String),
    DhcpError(String),
    DnsError(String),
    InterfaceNotFound(String),
    InvalidConfig(String),
}

impl std::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkError::ConfigError(msg) => write!(f, "config error: {msg}"),
            NetworkError::DhcpError(msg) => write!(f, "DHCP error: {msg}"),
            NetworkError::DnsError(msg) => write!(f, "DNS error: {msg}"),
            NetworkError::InterfaceNotFound(name) => write!(f, "interface not found: {name}"),
            NetworkError::InvalidConfig(msg) => write!(f, "invalid config: {msg}"),
        }
    }
}

impl std::error::Error for NetworkError {}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkConfig {
    #[serde(default = "default_dhcp")]
    pub dhcp: bool,
    #[serde(default)]
    pub static_config: Option<StaticIpConfig>,
    #[serde(default)]
    pub dns_servers: Vec<String>,
    #[serde(default)]
    pub dhcp_lease_file: Option<String>,
}

fn default_dhcp() -> bool { true }

impl Default for NetworkConfig {
    fn default() -> Self {
        Self { dhcp: true, static_config: None, dns_servers: vec![], dhcp_lease_file: None }
    }
}

impl NetworkConfig {
    pub fn from_toml(input: &str) -> Result<Self, NetworkError> {
        let cfg: NetworkConfigFile =
            toml::from_str(input).map_err(|e| NetworkError::InvalidConfig(e.to_string()))?;
        Ok(cfg.network)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct NetworkConfigFile {
    network: NetworkConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticIpConfig {
    pub address: String,
    pub gateway: String,
    pub subnet_mask: String,
}

// ---------------------------------------------------------------------------
// Interface state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceState {
    Down,
    Up { ip: Ipv4Addr, gateway: Ipv4Addr, subnet_mask: Ipv4Addr, dns: Vec<Ipv4Addr> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceInfo {
    pub name: String,
    pub state: InterfaceState,
    pub mac_address: String,
}

// ---------------------------------------------------------------------------
// DHCP lease & parser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DhcpLease {
    pub ip: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub subnet_mask: Ipv4Addr,
    pub dns_servers: Vec<Ipv4Addr>,
    pub lease_seconds: u32,
}

/// Parse a DHCP OFFER/ACK packet body using smoltcp's `DhcpRepr`.
pub fn parse_dhcp_offer(packet: &[u8]) -> Result<DhcpLease, NetworkError> {
    let dhcp_packet = DhcpPacket::new_checked(packet)
        .map_err(|_| NetworkError::DhcpError("invalid DHCP packet".into()))?;

    let repr = DhcpRepr::parse(&dhcp_packet)
        .map_err(|e| NetworkError::DhcpError(format!("DHCP parse failed: {e}")))?;

    let to_ipv4 = |addr: &Ipv4Address| Ipv4Addr::new(
        addr.as_bytes()[0], addr.as_bytes()[1],
        addr.as_bytes()[2], addr.as_bytes()[3],
    );

    let ip = to_ipv4(&repr.your_ip);
    let gateway = repr.router.as_ref().map(to_ipv4).unwrap_or(Ipv4Addr::UNSPECIFIED);
    let subnet_mask = repr.subnet_mask.as_ref().map(to_ipv4)
        .unwrap_or(Ipv4Addr::from([255, 255, 255, 0]));
    let dns_servers: Vec<Ipv4Addr> = repr.dns_servers
        .as_ref()
        .map(|v| v.iter().map(to_ipv4).collect())
        .unwrap_or_default();
    let lease_seconds = repr.lease_duration.unwrap_or(3600);

    Ok(DhcpLease { ip, gateway, subnet_mask, dns_servers, lease_seconds })
}

// ---------------------------------------------------------------------------
// DNS resolution
// ---------------------------------------------------------------------------

pub trait DnsTransport {
    fn send_recv(&mut self, query: &[u8]) -> Result<Vec<u8>, NetworkError>;
}

pub struct UdpDnsTransport {
    server: Ipv4Addr,
    port: u16,
}

impl UdpDnsTransport {
    pub fn new(server: Ipv4Addr, port: u16) -> Self {
        Self { server, port }
    }
}

impl DnsTransport for UdpDnsTransport {
    fn send_recv(&mut self, query: &[u8]) -> Result<Vec<u8>, NetworkError> {
        use std::net::UdpSocket;
        let socket = UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| NetworkError::DnsError(format!("bind failed: {e}")))?;
        socket.send_to(query, (self.server, self.port))
            .map_err(|e| NetworkError::DnsError(format!("send failed: {e}")))?;
        let mut buf = vec![0u8; 4096];
        let n = socket.recv_from(&mut buf)
            .map_err(|e| NetworkError::DnsError(format!("recv failed: {e}")))?;
        buf.truncate(n.0);
        Ok(buf)
    }
}

pub struct DnsResolver {
    transport: Box<dyn DnsTransport>,
}

impl DnsResolver {
    pub fn new(transport: Box<dyn DnsTransport>) -> Self {
        Self { transport }
    }

    pub fn resolve_a(&mut self, hostname: &str) -> Result<Ipv4Addr, NetworkError> {
        let query = build_a_record_query(hostname)?;
        let response = self.transport.send_recv(&query)?;
        parse_a_record_response(&response)
    }
}

/// Build a DNS A-record query packet manually.
fn build_a_record_query(hostname: &str) -> Result<Vec<u8>, NetworkError> {
    let mut buf = Vec::with_capacity(512);

    // DNS header (12 bytes)
    buf.extend_from_slice(&[0x00, 0x01]); // id = 1
    buf.extend_from_slice(&[0x01, 0x00]); // flags: recursion desired
    buf.extend_from_slice(&[0x00, 0x01]); // qdcount = 1
    buf.extend_from_slice(&[0x00, 0x00]); // ancount = 0
    buf.extend_from_slice(&[0x00, 0x00]); // nscount = 0
    buf.extend_from_slice(&[0x00, 0x00]); // arcount = 0

    // Encode the hostname as DNS labels
    for label in hostname.split('.') {
        if label.is_empty() {
            continue;
        }
        let len = label.len().min(63) as u8;
        buf.push(len);
        buf.extend_from_slice(label.as_bytes());
    }
    buf.push(0x00); // root label

    // QTYPE = A (1), QCLASS = IN (1)
    buf.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);

    Ok(buf)
}

/// Parse a DNS response and extract the first A-record IPv4 address.
fn parse_a_record_response(response: &[u8]) -> Result<Ipv4Addr, NetworkError> {
    let packet = DnsPacket::new_unchecked(response);
    if packet.answer_record_count() == 0 {
        return Err(NetworkError::DnsError("no answers in response".into()));
    }
    // packet.check_len() already verified at least header is valid

    // Manually walk past the header + questions to reach answer records.
    let mut offset = 12usize; // DNS header is always 12 bytes

    // Skip the question section
    let qcount = packet.question_count() as usize;
    for _ in 0..qcount {
        // Skip name — handles both label sequences and pointers
        loop {
            if offset >= response.len() {
                return Err(NetworkError::DnsError("truncated question".into()));
            }
            if response[offset] & 0xC0 == 0xC0 {
                offset += 2; // pointer
                break;
            }
            if response[offset] == 0 {
                offset += 1; // root label
                break;
            }
            let len = response[offset] as usize;
            offset += 1 + len;
        }
        offset += 4; // skip QTYPE + QCLASS
    }

    // Parse answer records
    let acount = packet.answer_record_count() as usize;
    for _ in 0..acount {
        // Skip name — handles both label sequences and pointers
        loop {
            if offset >= response.len() {
                return Err(NetworkError::DnsError("truncated answer".into()));
            }
            if response[offset] & 0xC0 == 0xC0 {
                offset += 2; // pointer
                break;
            }
            if response[offset] == 0 {
                offset += 1; // root label
                break;
            }
            let len = response[offset] as usize;
            offset += 1 + len;
        }

        if offset + 10 > response.len() {
            break;
        }
        let rtype = u16::from_be_bytes([response[offset], response[offset + 1]]);
        offset += 2;
        offset += 2; // class
        offset += 4; // TTL
        let rdlen = u16::from_be_bytes([response[offset], response[offset + 1]]) as usize;
        offset += 2;

        if offset + rdlen > response.len() {
            break;
        }

        // type 1 = A record
        if rtype == 1 && rdlen == 4 {
            return Ok(Ipv4Addr::new(
                response[offset], response[offset + 1],
                response[offset + 2], response[offset + 3],
            ));
        }
        offset += rdlen;
    }

    Err(NetworkError::DnsError("no A record found".into()))
}

// ---------------------------------------------------------------------------
// Mock DHCP client for testing
// ---------------------------------------------------------------------------

pub struct MockDhcpClient {
    lease: Option<DhcpLease>,
}

impl MockDhcpClient {
    pub fn new(lease: Option<DhcpLease>) -> Self {
        Self { lease }
    }

    pub fn discover(&mut self) -> Result<DhcpLease, NetworkError> {
        self.lease.clone().ok_or_else(|| NetworkError::DhcpError("no lease".into()))
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use smoltcp::wire::EthernetAddress;

    // ── 45.2: DHCP OFFER parsing ────────────────────────────────────────────

    fn to_ipv4_addr(s: Ipv4Addr) -> Ipv4Address {
        Ipv4Address(s.octets())
    }

    fn build_dhcp_offer_packet(
        yiaddr: Ipv4Addr,
        router: Option<Ipv4Addr>,
        mask: Option<Ipv4Addr>,
        dns: &[Ipv4Addr],
        lease_secs: u32,
    ) -> Vec<u8> {
        let mut dns_vec: heapless::Vec<Ipv4Address, { smoltcp::wire::DHCP_MAX_DNS_SERVER_COUNT as usize }> =
            heapless::Vec::new();
        for a in dns {
            dns_vec.push(Ipv4Address(a.octets())).ok();
        }

        let repr = DhcpRepr {
            message_type: smoltcp::wire::DhcpMessageType::Offer,
            transaction_id: 0x12345678,
            secs: 0,
            broadcast: false,
            client_hardware_address: EthernetAddress([0u8; 6]),
            client_ip: Ipv4Address::UNSPECIFIED,
            your_ip: to_ipv4_addr(yiaddr),
            server_ip: Ipv4Address::UNSPECIFIED,
            relay_agent_ip: Ipv4Address::UNSPECIFIED,
            router: router.map(to_ipv4_addr),
            subnet_mask: mask.map(to_ipv4_addr),
            dns_servers: if dns_vec.is_empty() { None } else { Some(dns_vec) },
            requested_ip: None,
            client_identifier: None,
            server_identifier: None,
            parameter_request_list: None,
            max_size: None,
            lease_duration: Some(lease_secs),
            renew_duration: None,
            rebind_duration: None,
            additional_options: &[],
        };

        let mut buf = vec![0u8; repr.buffer_len()];
        let mut packet = DhcpPacket::new_unchecked(&mut buf);
        let _ = repr.emit(&mut packet);
        buf
    }

    #[test]
    fn test_dhcp_offer_parses_ip() {
        let packet = build_dhcp_offer_packet(
            Ipv4Addr::new(192, 168, 1, 100),
            Some(Ipv4Addr::new(192, 168, 1, 1)),
            Some(Ipv4Addr::new(255, 255, 255, 0)),
            &[Ipv4Addr::new(8, 8, 8, 8)], 3600,
        );
        let lease = parse_dhcp_offer(&packet).unwrap();
        assert_eq!(lease.ip, Ipv4Addr::new(192, 168, 1, 100));
    }

    #[test]
    fn test_dhcp_offer_parses_gateway() {
        let packet = build_dhcp_offer_packet(
            Ipv4Addr::new(10, 0, 0, 50),
            Some(Ipv4Addr::new(10, 0, 0, 1)),
            Some(Ipv4Addr::new(255, 0, 0, 0)),
            &[], 7200,
        );
        let lease = parse_dhcp_offer(&packet).unwrap();
        assert_eq!(lease.gateway, Ipv4Addr::new(10, 0, 0, 1));
    }

    #[test]
    fn test_dhcp_offer_parses_subnet_mask() {
        let packet = build_dhcp_offer_packet(
            Ipv4Addr::new(172, 16, 0, 10),
            Some(Ipv4Addr::new(172, 16, 0, 1)),
            Some(Ipv4Addr::new(255, 255, 0, 0)),
            &[], 1800,
        );
        let lease = parse_dhcp_offer(&packet).unwrap();
        assert_eq!(lease.subnet_mask, Ipv4Addr::new(255, 255, 0, 0));
    }

    #[test]
    fn test_dhcp_offer_parses_dns_servers() {
        let packet = build_dhcp_offer_packet(
            Ipv4Addr::new(192, 168, 1, 100),
            Some(Ipv4Addr::new(192, 168, 1, 1)),
            Some(Ipv4Addr::new(255, 255, 255, 0)),
            &[Ipv4Addr::new(8, 8, 8, 8), Ipv4Addr::new(8, 8, 4, 4)], 3600,
        );
        let lease = parse_dhcp_offer(&packet).unwrap();
        assert_eq!(lease.dns_servers.len(), 2);
        assert_eq!(lease.dns_servers[0], Ipv4Addr::new(8, 8, 8, 8));
        assert_eq!(lease.dns_servers[1], Ipv4Addr::new(8, 8, 4, 4));
    }

    #[test]
    fn test_dhcp_offer_parses_lease_time() {
        let packet = build_dhcp_offer_packet(
            Ipv4Addr::new(10, 0, 0, 5),
            Some(Ipv4Addr::new(10, 0, 0, 1)),
            Some(Ipv4Addr::new(255, 0, 0, 0)),
            &[], 86400,
        );
        let lease = parse_dhcp_offer(&packet).unwrap();
        assert_eq!(lease.lease_seconds, 86400);
    }

    #[test]
    fn test_dhcp_offer_no_optional_fields() {
        let packet = build_dhcp_offer_packet(
            Ipv4Addr::new(10, 0, 0, 5),
            None, None, &[], 3600,
        );
        let lease = parse_dhcp_offer(&packet).unwrap();
        assert_eq!(lease.ip, Ipv4Addr::new(10, 0, 0, 5));
    }

    #[test]
    fn test_invalid_dhcp_packet_rejected() {
        assert!(parse_dhcp_offer(b"nope").is_err());
    }

    // ── Config parsing ──────────────────────────────────────────────────────

    #[test]
    fn test_network_config_defaults() {
        let cfg = NetworkConfig::from_toml("[network]\n").unwrap();
        assert!(cfg.dhcp);
    }

    #[test]
    fn test_static_ip_config() {
        let toml = r#"
[network]
dhcp = false
[network.static_config]
address = "192.168.1.100"
gateway = "192.168.1.1"
subnet_mask = "255.255.255.0"
"#;
        let cfg = NetworkConfig::from_toml(toml).unwrap();
        assert!(!cfg.dhcp);
        let s = cfg.static_config.unwrap();
        assert_eq!(s.address, "192.168.1.100");
    }

    #[test]
    fn test_invalid_toml_rejected() {
        assert!(NetworkConfig::from_toml("not valid {{{").is_err());
    }

    // ── Mock DHCP client ────────────────────────────────────────────────────

    #[test]
    fn test_mock_dhcp_client() {
        let lease = DhcpLease {
            ip: Ipv4Addr::new(10, 0, 0, 42),
            gateway: Ipv4Addr::new(10, 0, 0, 1),
            subnet_mask: Ipv4Addr::new(255, 0, 0, 0),
            dns_servers: vec![Ipv4Addr::new(10, 0, 0, 53)],
            lease_seconds: 3600,
        };
        let mut c = MockDhcpClient::new(Some(lease.clone()));
        assert_eq!(c.discover().unwrap(), lease);
        let mut c = MockDhcpClient::new(None);
        assert!(c.discover().is_err());
    }

    // ── DNS ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_build_dns_query() {
        let q = build_a_record_query("example.com").unwrap();
        assert!(q.len() > 12); // header + question
        let packet = DnsPacket::new_unchecked(&q);
        assert_eq!(packet.question_count(), 1);
    }

    #[test]
    fn test_parse_dns_a_response() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0x00, 0x01, 0x81, 0x80]); // id, flags
        buf.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]); // 1 question, 1 answer
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // 0 auth, 0 additional
        // question: example.com A IN
        buf.extend_from_slice(b"\x07example\x03com\x00");
        buf.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);
        // answer: pointer 0xc00c, type A, class IN, TTL=300, len=4, data
        buf.extend_from_slice(&[0xc0, 0x0c]);
        buf.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);
        buf.extend_from_slice(&[0x00, 0x00, 0x01, 0x2c]); // TTL=300
        buf.extend_from_slice(&[0x00, 0x04]); // rdlen=4
        buf.extend_from_slice(&[93, 184, 216, 34]); // 93.184.216.34

        let addr = parse_a_record_response(&buf).unwrap();
        assert_eq!(addr, Ipv4Addr::new(93, 184, 216, 34));
    }

    #[test]
    fn test_parse_dns_no_a_record() {
        let buf = vec![
            0x00, 0x01, 0x81, 0x83, // id, flags (NXDOMAIN)
            0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // counts
            0x00, 0x00, 0x01, 0x00, 0x01, // root question
        ];
        assert!(parse_a_record_response(&buf).is_err());
    }
}
