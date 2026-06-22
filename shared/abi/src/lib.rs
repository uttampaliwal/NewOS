#![cfg_attr(not(test), no_std)]

pub mod boot;
pub mod input;
pub mod syscall;
pub mod version;

/// Maximum number of network interfaces the kernel supports.
pub const NET_MAX_INTERFACES: u32 = 8;

/// Request structure for `NetSetAddr` syscall.
/// Sets the IP address, netmask, and gateway for a given interface.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetSetAddrReq {
    pub iface_id: u32,
    pub addr: [u8; 4],
    pub netmask: [u8; 4],
    pub gateway: [u8; 4],
}

/// Request structure for `NetSetRoute` syscall.
/// Sets the default gateway for interface 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetSetRouteReq {
    pub gateway: [u8; 4],
}

/// Response structure for `NetQuery` syscall.
/// Returns the current configuration of an interface.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetQueryResp {
    pub ip: [u8; 4],
    pub netmask: [u8; 4],
    pub gateway: [u8; 4],
    pub mtu: u16,
    pub flags: u16,
}
