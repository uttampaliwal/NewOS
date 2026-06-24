pub mod smoltcp_iface;
pub mod socket;

use spin::Mutex;

/// Per-interface network configuration stored in the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetInterfaceConfig {
    pub ip: [u8; 4],
    pub netmask: [u8; 4],
    pub gateway: [u8; 4],
    pub mtu: u16,
    pub up: bool,
}

impl Default for NetInterfaceConfig {
    fn default() -> Self {
        Self {
            ip: [0; 4],
            netmask: [0; 4],
            gateway: [0; 4],
            mtu: 1500,
            up: false,
        }
    }
}

/// Global array of interface configurations (max 8 interfaces).
pub static NET_CONFIGS: Mutex<[NetInterfaceConfig; 8]> = Mutex::new(
    [NetInterfaceConfig {
        ip: [0; 4],
        netmask: [0; 4],
        gateway: [0; 4],
        mtu: 1500,
        up: false,
    }; 8],
);
