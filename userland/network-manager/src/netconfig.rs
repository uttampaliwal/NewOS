use std::net::Ipv4Addr;

use crate::dhcp;
use crate::NetworkError;

/// Network interface configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetConfig {
    pub interface: String,
    pub ip: Ipv4Addr,
    pub subnet_mask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub dns: Vec<Ipv4Addr>,
    pub mtu: u32,
    pub up: bool,
}

/// Trait for controlling network interfaces.
pub trait InterfaceController {
    fn set_ip(&mut self, name: &str, ip: Ipv4Addr, mask: Ipv4Addr) -> Result<(), NetworkError>;
    fn set_gateway(&mut self, name: &str, gw: Ipv4Addr) -> Result<(), NetworkError>;
    fn add_dns(&mut self, name: &str, dns: Ipv4Addr) -> Result<(), NetworkError>;
    fn interface_up(&mut self, name: &str) -> Result<(), NetworkError>;
    fn interface_down(&mut self, name: &str) -> Result<(), NetworkError>;
}

impl NetConfig {
    /// Create configuration from a DHCP lease.
    pub fn from_dhcp(lease: &dhcp::DhcpLease, interface: &str) -> Self {
        Self {
            interface: interface.to_string(),
            ip: lease.ip,
            subnet_mask: lease.subnet_mask,
            gateway: lease.gateway,
            dns: lease.dns_servers.clone(),
            mtu: 1500,
            up: true,
        }
    }

    /// Create a static configuration.
    pub fn static_config(
        interface: &str,
        ip: Ipv4Addr,
        subnet_mask: Ipv4Addr,
        gateway: Ipv4Addr,
        dns: Vec<Ipv4Addr>,
    ) -> Self {
        Self {
            interface: interface.to_string(),
            ip,
            subnet_mask,
            gateway,
            dns,
            mtu: 1500,
            up: true,
        }
    }

    /// Apply this configuration (configure the interface).
    pub fn apply(&self, controller: &mut dyn InterfaceController) -> Result<(), NetworkError> {
        controller.set_ip(&self.interface, self.ip, self.subnet_mask)?;
        controller.set_gateway(&self.interface, self.gateway)?;
        for dns in &self.dns {
            controller.add_dns(&self.interface, *dns)?;
        }
        if self.up {
            controller.interface_up(&self.interface)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockController;

    impl InterfaceController for MockController {
        fn set_ip(&mut self, _name: &str, _ip: Ipv4Addr, _mask: Ipv4Addr) -> Result<(), NetworkError> {
            Ok(())
        }
        fn set_gateway(&mut self, _name: &str, _gw: Ipv4Addr) -> Result<(), NetworkError> {
            Ok(())
        }
        fn add_dns(&mut self, _name: &str, _dns: Ipv4Addr) -> Result<(), NetworkError> {
            Ok(())
        }
        fn interface_up(&mut self, _name: &str) -> Result<(), NetworkError> {
            Ok(())
        }
        fn interface_down(&mut self, _name: &str) -> Result<(), NetworkError> {
            Ok(())
        }
    }

    #[test]
    fn test_from_dhcp() {
        let lease = dhcp::DhcpLease {
            ip: Ipv4Addr::new(192, 168, 1, 100),
            subnet_mask: Ipv4Addr::new(255, 255, 255, 0),
            gateway: Ipv4Addr::new(192, 168, 1, 1),
            dns_servers: vec![Ipv4Addr::new(8, 8, 8, 8)],
            lease_seconds: 86400,
            server_ip: Ipv4Addr::new(192, 168, 1, 1),
        };
        let cfg = NetConfig::from_dhcp(&lease, "eth0");
        assert_eq!(cfg.ip, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(cfg.gateway, Ipv4Addr::new(192, 168, 1, 1));
        assert!(cfg.up);
    }

    #[test]
    fn test_static_config() {
        let cfg = NetConfig::static_config(
            "eth0",
            Ipv4Addr::new(10, 0, 0, 50),
            Ipv4Addr::new(255, 0, 0, 0),
            Ipv4Addr::new(10, 0, 0, 1),
            vec![Ipv4Addr::new(1, 1, 1, 1)],
        );
        assert_eq!(cfg.ip, Ipv4Addr::new(10, 0, 0, 50));
        assert_eq!(cfg.mtu, 1500);
    }

    #[test]
    fn test_apply() {
        let cfg = NetConfig::static_config(
            "eth0",
            Ipv4Addr::new(10, 0, 0, 50),
            Ipv4Addr::new(255, 0, 0, 0),
            Ipv4Addr::new(10, 0, 0, 1),
            vec![Ipv4Addr::new(1, 1, 1, 1)],
        );
        let mut ctrl = MockController;
        assert!(cfg.apply(&mut ctrl).is_ok());
    }
}
