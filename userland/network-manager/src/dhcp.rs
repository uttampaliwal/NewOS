//! DHCP client protocol implementation.
//!
//! Implements the DHCP DISCOVER/OFFER/REQUEST/ACK state machine
//! for obtaining IP configuration from a DHCP server.

use std::net::Ipv4Addr;

/// DHCP message types (DHCP Message Type option 53).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DhcpMessageType {
    Discover = 1,
    Offer = 2,
    Request = 3,
    Decline = 4,
    Ack = 5,
    Nak = 6,
    Release = 7,
    Inform = 8,
}

/// DHCP option codes.
pub mod options {
    pub const PAD: u8 = 0;
    pub const SUBNET_MASK: u8 = 1;
    pub const ROUTER: u8 = 3;
    pub const DNS_SERVER: u8 = 6;
    pub const DOMAIN_NAME: u8 = 15;
    pub const BROADCAST_ADDR: u8 = 28;
    pub const REQUESTED_IP: u8 = 50;
    pub const LEASE_TIME: u8 = 51;
    pub const MESSAGE_TYPE: u8 = 53;
    pub const SERVER_ID: u8 = 54;
    pub const PARAM_REQUEST_LIST: u8 = 55;
    pub const CLIENT_ID: u8 = 61;
    pub const END: u8 = 255;
}

/// A DHCP lease obtained from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DhcpLease {
    pub ip: Ipv4Addr,
    pub subnet_mask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub dns_servers: Vec<Ipv4Addr>,
    pub lease_seconds: u32,
    pub server_ip: Ipv4Addr,
}

/// DHCP client state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DhcpState {
    Init,
    Selecting,
    Requesting,
    Bound,
    Renewing,
    Rebinding,
}

/// DHCP client that manages the state machine.
pub struct DhcpClient {
    state: DhcpState,
    client_ip: Ipv4Addr,
    server_ip: Option<Ipv4Addr>,
    transaction_id: u32,
    lease: Option<DhcpLease>,
}

impl DhcpClient {
    pub fn new() -> Self {
        Self {
            state: DhcpState::Init,
            client_ip: Ipv4Addr::UNSPECIFIED,
            server_ip: None,
            transaction_id: rand_xid(),
            lease: None,
        }
    }

    /// Build a DHCP DISCOVER packet.
    pub fn build_discover(&mut self) -> DhcpPacket {
        self.transaction_id = rand_xid();
        self.state = DhcpState::Selecting;

        let mut pkt = DhcpPacket::new(DhcpMessageType::Discover, self.transaction_id);
        pkt.ciaddr = self.client_ip;
        pkt.add_option(options::PARAM_REQUEST_LIST, &[1, 3, 6, 15, 51]);
        pkt.add_option(options::CLIENT_ID, &[0x01]);
        pkt.finalize();
        pkt
    }

    /// Process a DHCP OFFER and build a REQUEST.
    pub fn handle_offer(&mut self, offer: &DhcpPacket) -> Option<DhcpPacket> {
        let server_ip = offer.server_id()?;
        self.server_ip = Some(server_ip);

        let offered_ip = if offer.yiaddr != Ipv4Addr::UNSPECIFIED {
            offer.yiaddr
        } else {
            return None;
        };

        self.state = DhcpState::Requesting;

        let mut pkt = DhcpPacket::new(DhcpMessageType::Request, self.transaction_id);
        pkt.ciaddr = self.client_ip;
        pkt.add_option(options::REQUESTED_IP, &offered_ip.octets());
        pkt.add_option(options::SERVER_ID, &server_ip.octets());
        pkt.add_option(options::PARAM_REQUEST_LIST, &[1, 3, 6, 15, 51]);
        pkt.finalize();
        Some(pkt)
    }

    /// Process a DHCP ACK.
    pub fn handle_ack(&mut self, ack: &DhcpPacket) -> Option<DhcpLease> {
        let ip = if ack.yiaddr != Ipv4Addr::UNSPECIFIED {
            ack.yiaddr
        } else {
            self.client_ip
        };

        let subnet_mask = ack.subnet_mask().unwrap_or(Ipv4Addr::new(255, 255, 255, 0));
        let gateway = ack.router().unwrap_or(Ipv4Addr::new(192, 168, 1, 1));
        let dns_servers = ack.dns_servers();
        let lease_time = ack.lease_time().unwrap_or(86400);
        let server_ip = self.server_ip.unwrap_or(Ipv4Addr::UNSPECIFIED);

        self.client_ip = ip;
        self.state = DhcpState::Bound;

        let lease = DhcpLease {
            ip,
            subnet_mask,
            gateway,
            dns_servers,
            lease_seconds: lease_time,
            server_ip,
        };
        self.lease = Some(lease.clone());
        Some(lease)
    }

    /// Process a DHCP NAK.
    pub fn handle_nak(&mut self) {
        self.state = DhcpState::Init;
        self.server_ip = None;
        self.lease = None;
    }

    /// Build a DHCP RELEASE.
    pub fn build_release(&self) -> Option<DhcpPacket> {
        let server_ip = self.server_ip?;
        let mut pkt = DhcpPacket::new(DhcpMessageType::Release, self.transaction_id);
        pkt.ciaddr = self.client_ip;
        pkt.add_option(options::SERVER_ID, &server_ip.octets());
        pkt.finalize();
        Some(pkt)
    }

    pub fn state(&self) -> DhcpState {
        self.state
    }

    pub fn client_ip(&self) -> Ipv4Addr {
        self.client_ip
    }

    pub fn lease(&self) -> Option<&DhcpLease> {
        self.lease.as_ref()
    }
}

impl Default for DhcpClient {
    fn default() -> Self {
        Self::new()
    }
}

/// A DHCP packet (simplified BOOTP format).
#[derive(Debug, Clone)]
pub struct DhcpPacket {
    pub op: u8,
    pub htype: u8,
    pub hlen: u8,
    pub hops: u8,
    pub xid: u32,
    pub secs: u16,
    pub flags: u16,
    pub ciaddr: Ipv4Addr,
    pub yiaddr: Ipv4Addr,
    pub siaddr: Ipv4Addr,
    pub giaddr: Ipv4Addr,
    pub chaddr: [u8; 16],
    pub sname: [u8; 64],
    pub file: [u8; 128],
    pub magic: u32,
    pub options: Vec<u8>,
}

impl DhcpPacket {
    pub fn new(msg_type: DhcpMessageType, xid: u32) -> Self {
        let mut pkt = Self {
            op: 1,
            htype: 1,
            hlen: 6,
            hops: 0,
            xid,
            secs: 0,
            flags: 0x8000,
            ciaddr: Ipv4Addr::UNSPECIFIED,
            yiaddr: Ipv4Addr::UNSPECIFIED,
            siaddr: Ipv4Addr::UNSPECIFIED,
            giaddr: Ipv4Addr::UNSPECIFIED,
            chaddr: [0u8; 16],
            sname: [0u8; 64],
            file: [0u8; 128],
            magic: 0x63825363,
            options: Vec::new(),
        };
        pkt.add_option(options::MESSAGE_TYPE, &[msg_type as u8]);
        pkt
    }

    pub fn add_option(&mut self, code: u8, data: &[u8]) {
        self.options.push(code);
        self.options.push(data.len() as u8);
        self.options.extend_from_slice(data);
    }

    pub fn finalize(&mut self) {
        self.options.push(options::END);
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(240 + self.options.len());
        buf.push(self.op);
        buf.push(self.htype);
        buf.push(self.hlen);
        buf.push(self.hops);
        buf.extend_from_slice(&self.xid.to_be_bytes());
        buf.extend_from_slice(&self.secs.to_be_bytes());
        buf.extend_from_slice(&self.flags.to_be_bytes());
        buf.extend_from_slice(&self.ciaddr.octets());
        buf.extend_from_slice(&self.yiaddr.octets());
        buf.extend_from_slice(&self.siaddr.octets());
        buf.extend_from_slice(&self.giaddr.octets());
        buf.extend_from_slice(&self.chaddr);
        buf.extend_from_slice(&self.sname);
        buf.extend_from_slice(&self.file);
        buf.extend_from_slice(&self.magic.to_be_bytes());
        buf.extend_from_slice(&self.options);
        buf
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 240 {
            return None;
        }

        let magic = u32::from_be_bytes([data[236], data[237], data[238], data[239]]);
        if magic != 0x63825363 {
            return None;
        }

        let mut chaddr = [0u8; 16];
        chaddr.copy_from_slice(&data[28..44]);

        let pkt = Self {
            op: data[0],
            htype: data[1],
            hlen: data[2],
            hops: data[3],
            xid: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            secs: u16::from_be_bytes([data[8], data[9]]),
            flags: u16::from_be_bytes([data[10], data[11]]),
            ciaddr: Ipv4Addr::new(data[12], data[13], data[14], data[15]),
            yiaddr: Ipv4Addr::new(data[16], data[17], data[18], data[19]),
            siaddr: Ipv4Addr::new(data[20], data[21], data[22], data[23]),
            giaddr: Ipv4Addr::new(data[24], data[25], data[26], data[27]),
            chaddr,
            sname: {
                let mut s = [0u8; 64];
                s.copy_from_slice(&data[44..108]);
                s
            },
            file: {
                let mut f = [0u8; 128];
                f.copy_from_slice(&data[108..236]);
                f
            },
            magic,
            options: data[240..].to_vec(),
        };

        Some(pkt)
    }

    pub fn get_option(&self, code: u8) -> Option<&[u8]> {
        let mut i = 0;
        while i < self.options.len() {
            let opt_code = self.options[i];
            if opt_code == options::END {
                break;
            }
            if opt_code == options::PAD {
                i += 1;
                continue;
            }
            if i + 1 >= self.options.len() {
                break;
            }
            let opt_len = self.options[i + 1] as usize;
            if opt_code == code {
                return Some(&self.options[i + 2..i + 2 + opt_len]);
            }
            i += 2 + opt_len;
        }
        None
    }

    pub fn message_type(&self) -> Option<DhcpMessageType> {
        let data = self.get_option(options::MESSAGE_TYPE)?;
        if data.is_empty() {
            return None;
        }
        match data[0] {
            1 => Some(DhcpMessageType::Discover),
            2 => Some(DhcpMessageType::Offer),
            3 => Some(DhcpMessageType::Request),
            4 => Some(DhcpMessageType::Decline),
            5 => Some(DhcpMessageType::Ack),
            6 => Some(DhcpMessageType::Nak),
            7 => Some(DhcpMessageType::Release),
            8 => Some(DhcpMessageType::Inform),
            _ => None,
        }
    }

    pub fn server_id(&self) -> Option<Ipv4Addr> {
        let data = self.get_option(options::SERVER_ID)?;
        if data.len() < 4 {
            return None;
        }
        Some(Ipv4Addr::new(data[0], data[1], data[2], data[3]))
    }

    pub fn subnet_mask(&self) -> Option<Ipv4Addr> {
        let data = self.get_option(options::SUBNET_MASK)?;
        if data.len() < 4 {
            return None;
        }
        Some(Ipv4Addr::new(data[0], data[1], data[2], data[3]))
    }

    pub fn router(&self) -> Option<Ipv4Addr> {
        let data = self.get_option(options::ROUTER)?;
        if data.len() < 4 {
            return None;
        }
        Some(Ipv4Addr::new(data[0], data[1], data[2], data[3]))
    }

    pub fn dns_servers(&self) -> Vec<Ipv4Addr> {
        let data = match self.get_option(options::DNS_SERVER) {
            Some(d) => d,
            None => return Vec::new(),
        };
        data.chunks(4)
            .filter(|c| c.len() == 4)
            .map(|c| Ipv4Addr::new(c[0], c[1], c[2], c[3]))
            .collect()
    }

    pub fn lease_time(&self) -> Option<u32> {
        let data = self.get_option(options::LEASE_TIME)?;
        if data.len() < 4 {
            return None;
        }
        Some(u32::from_be_bytes([data[0], data[1], data[2], data[3]]))
    }
}

fn rand_xid() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    t.as_nanos() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_packet() {
        let mut client = DhcpClient::new();
        let pkt = client.build_discover();
        assert_eq!(pkt.op, 1);
        assert_eq!(pkt.message_type(), Some(DhcpMessageType::Discover));
    }

    #[test]
    fn test_offer_to_request() {
        let mut client = DhcpClient::new();
        client.build_discover();

        let mut offer = DhcpPacket::new(DhcpMessageType::Offer, client.transaction_id);
        offer.yiaddr = Ipv4Addr::new(192, 168, 1, 100);
        offer.add_option(options::SERVER_ID, &Ipv4Addr::new(192, 168, 1, 1).octets());
        offer.add_option(options::SUBNET_MASK, &Ipv4Addr::new(255, 255, 255, 0).octets());
        offer.add_option(options::ROUTER, &Ipv4Addr::new(192, 168, 1, 1).octets());
        offer.add_option(options::DNS_SERVER, &Ipv4Addr::new(8, 8, 8, 8).octets());
        offer.add_option(options::LEASE_TIME, &86400u32.to_be_bytes());
        offer.finalize();

        let req = client.handle_offer(&offer).unwrap();
        assert_eq!(req.message_type(), Some(DhcpMessageType::Request));
    }

    #[test]
    fn test_ack_lease() {
        let mut client = DhcpClient::new();
        client.build_discover();

        let mut ack = DhcpPacket::new(DhcpMessageType::Ack, client.transaction_id);
        ack.yiaddr = Ipv4Addr::new(10, 0, 0, 50);
        ack.add_option(options::SUBNET_MASK, &Ipv4Addr::new(255, 255, 255, 0).octets());
        ack.add_option(options::ROUTER, &Ipv4Addr::new(10, 0, 0, 1).octets());
        ack.add_option(options::DNS_SERVER, &Ipv4Addr::new(1, 1, 1, 1).octets());
        ack.add_option(options::LEASE_TIME, &3600u32.to_be_bytes());
        ack.finalize();

        let lease = client.handle_ack(&ack).unwrap();
        assert_eq!(lease.ip, Ipv4Addr::new(10, 0, 0, 50));
        assert_eq!(lease.lease_seconds, 3600);
        assert_eq!(client.state(), DhcpState::Bound);
    }

    #[test]
    fn test_serialize_parse_roundtrip() {
        let mut client = DhcpClient::new();
        let pkt = client.build_discover();
        let bytes = pkt.to_bytes();
        let parsed = DhcpPacket::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.xid, pkt.xid);
        assert_eq!(parsed.message_type(), Some(DhcpMessageType::Discover));
    }

    #[test]
    fn test_nak_resets_state() {
        let mut client = DhcpClient::new();
        client.build_discover();
        client.state = DhcpState::Requesting;
        client.handle_nak();
        assert_eq!(client.state(), DhcpState::Init);
    }

    #[test]
    fn test_invalid_packet_returns_none() {
        assert!(DhcpPacket::from_bytes(&[0u8; 100]).is_none());
    }

    #[test]
    fn test_release_packet() {
        let mut client = DhcpClient::new();
        client.build_discover();
        client.server_ip = Some(Ipv4Addr::new(192, 168, 1, 1));
        let release = client.build_release().unwrap();
        assert_eq!(release.message_type(), Some(DhcpMessageType::Release));
    }

    #[test]
    fn test_no_server_id_returns_none() {
        let pkt = DhcpPacket::new(DhcpMessageType::Offer, 123);
        assert!(pkt.server_id().is_none());
    }
}
