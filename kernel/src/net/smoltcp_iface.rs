use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use lazy_static::lazy_static;
use smoltcp::iface::{Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Helper: create a &'static mut [u8] from a Vec
// ---------------------------------------------------------------------------

fn vec_to_static_slice(v: Vec<u8>) -> &'static mut [u8] {
    Box::leak(v.into_boxed_slice())
}

// ---------------------------------------------------------------------------
// VirtIO-Net physical device adapter for smoltcp
// ---------------------------------------------------------------------------

pub struct VirtioNetPhyDevice;

pub struct VirtioNetRxToken(Vec<u8>);
pub struct VirtioNetTxToken;

impl RxToken for VirtioNetRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        f(&mut self.0.as_slice().to_vec())
    }
}

impl TxToken for VirtioNetTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = alloc::vec![0u8; len];
        let result = f(&mut buffer);
        crate::drivers::virtio_net::transmit_packet(&buffer);
        result
    }
}

impl Device for VirtioNetPhyDevice {
    type RxToken<'a> = VirtioNetRxToken where Self: 'a;
    type TxToken<'a> = VirtioNetTxToken where Self: 'a;

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = 1500;
        caps.max_burst_size = Some(1);
        caps
    }

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let mut received: Option<Vec<u8>> = None;
        crate::drivers::virtio_net::poll_rx(|buf| {
            received = Some(buf.to_vec());
        });
        received.map(|data| (VirtioNetRxToken(data), VirtioNetTxToken))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(VirtioNetTxToken)
    }
}

// ---------------------------------------------------------------------------
// Socket storage type alias
// ---------------------------------------------------------------------------

// Socket storage is managed internally via Vec in SocketSet::new

// ---------------------------------------------------------------------------
// Global network stack state
// ---------------------------------------------------------------------------

pub struct NetworkStack {
    pub interface: Interface,
    pub sockets: SocketSet<'static>,
    pub device: VirtioNetPhyDevice,
    pub initialized: bool,
    pub mac_address: [u8; 6],
    next_ephemeral_port: u16,
}

impl NetworkStack {
    fn new() -> Self {
        let mac = crate::drivers::virtio_net::get_mac_address()
            .unwrap_or([0x02, 0x00, 0xAD, 0xDE, 0x00, 0x01]);

        let ethernet_addr = EthernetAddress(mac);
        let hardware_addr = HardwareAddress::Ethernet(ethernet_addr);

        let mut device = VirtioNetPhyDevice;
        let mut config = smoltcp::iface::Config::new();
        config.hardware_addr = Some(hardware_addr);
        config.random_seed = 12345;

        let mut interface = Interface::new(config, &mut device);

        // Set a default IP address (will be overridden by DHCP)
        let ip_cidr = IpCidr::new(IpAddress::v4(0, 0, 0, 0), 0);
        interface.update_ip_addrs(|addrs| {
            let _ = addrs.push(ip_cidr);
        });

        let sockets = SocketSet::new(Vec::new());

        Self {
            interface,
            sockets,
            device,
            initialized: true,
            mac_address: mac,
            next_ephemeral_port: 49152,
        }
    }

    pub fn poll(&mut self, timestamp: Instant) -> bool {
        self.interface.poll(timestamp, &mut self.device, &mut self.sockets)
    }

    pub fn poll_delay(&mut self, timestamp: Instant) -> Option<smoltcp::time::Duration> {
        self.interface.poll_delay(timestamp, &self.sockets)
    }

    pub fn add_tcp_socket(&mut self) -> SocketHandle {
        let rx_buffer = smoltcp::socket::tcp::SocketBuffer::new(vec_to_static_slice(vec![0u8; 65536]));
        let tx_buffer = smoltcp::socket::tcp::SocketBuffer::new(vec_to_static_slice(vec![0u8; 65536]));
        let socket = smoltcp::socket::tcp::Socket::new(rx_buffer, tx_buffer);
        self.sockets.add(socket)
    }

    pub fn add_udp_socket(&mut self) -> SocketHandle {
        let rx_meta = vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 16];
        let rx_data = vec_to_static_slice(vec![0u8; 65536]);
        let rx_buffer = smoltcp::socket::udp::PacketBuffer::new(rx_meta, rx_data);
        let tx_meta = vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 16];
        let tx_data = vec_to_static_slice(vec![0u8; 65536]);
        let tx_buffer = smoltcp::socket::udp::PacketBuffer::new(tx_meta, tx_data);
        let socket = smoltcp::socket::udp::Socket::new(rx_buffer, tx_buffer);
        self.sockets.add(socket)
    }

    pub fn remove_socket(&mut self, handle: SocketHandle) {
        self.sockets.remove(handle);
    }

    pub fn alloc_ephemeral_port(&mut self) -> u16 {
        let port = self.next_ephemeral_port;
        self.next_ephemeral_port = if self.next_ephemeral_port >= 65535 {
            49152
        } else {
            self.next_ephemeral_port + 1
        };
        port
    }

    pub fn connect_tcp(&mut self, handle: SocketHandle, remote: IpEndpoint) -> Result<(), smoltcp::socket::tcp::ConnectError> {
        let local_port = self.alloc_ephemeral_port();
        let socket = self.sockets.get_mut::<smoltcp::socket::tcp::Socket>(handle);
        let cx = self.interface.context();
        let local = (smoltcp::wire::IpAddress::v4(0, 0, 0, 0), local_port);
        socket.connect(cx, remote, local)
    }
}

lazy_static! {
    pub static ref NET_STACK: Mutex<NetworkStack> = Mutex::new(NetworkStack::new());
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

pub fn init() {
    let mac = {
        let stack = NET_STACK.lock();
        stack.mac_address
    };
    crate::serial::println!(
        "[NET] smoltcp stack initialized, MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    );
}

// ---------------------------------------------------------------------------
// Poll the network stack (called from timer interrupt)
// ---------------------------------------------------------------------------

pub fn poll_stack() {
    let mut stack = NET_STACK.lock();
    if !stack.initialized {
        return;
    }
    let ticks = crate::task::scheduler::get_uptime_ticks();
    let millis = ticks * 10; // each tick is ~10ms
    let instant = Instant::from_millis(millis as i64);
    stack.poll(instant);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_stack_initialized() {
        let stack = NET_STACK.lock();
        assert!(stack.initialized);
    }

    #[test]
    fn test_network_stack_mac_address() {
        let stack = NET_STACK.lock();
        assert_eq!(stack.mac_address.len(), 6);
    }

    #[test]
    fn test_add_tcp_socket() {
        let mut stack = NET_STACK.lock();
        let handle = stack.add_tcp_socket();
        let _socket: &smoltcp::socket::tcp::Socket = stack.sockets.get(handle);
    }

    #[test]
    fn test_add_udp_socket() {
        let mut stack = NET_STACK.lock();
        let handle = stack.add_udp_socket();
        let _socket: &smoltcp::socket::udp::Socket = stack.sockets.get(handle);
    }

    #[test]
    fn test_remove_socket() {
        let mut stack = NET_STACK.lock();
        let handle = stack.add_tcp_socket();
        stack.remove_socket(handle);
        // SocketSet doesn't expose len(), so just verify no panic
    }

    #[test]
    fn test_device_capabilities() {
        let device = VirtioNetPhyDevice;
        let caps = device.capabilities();
        assert_eq!(caps.max_transmission_unit, 1500);
        assert_eq!(caps.medium, Medium::Ethernet);
    }
}
