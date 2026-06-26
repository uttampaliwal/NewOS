use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use spin::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkError {
    NotFound,
    AlreadyExists,
    InvalidConfig(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VethPair {
    pub host_name: String,
    pub container_name: String,
    pub host_idx: u32,
    pub container_idx: u32,
}

#[derive(Debug, Clone)]
pub struct Bridge {
    pub name: String,
    pub ip: [u8; 4],
    pub netmask: [u8; 4],
    pub interfaces: Vec<String>,
}

pub struct ContainerNetwork {
    veths: BTreeMap<String, VethPair>,
    bridges: BTreeMap<String, Bridge>,
    next_idx: u32,
}

impl ContainerNetwork {
    fn new() -> Self {
        Self {
            veths: BTreeMap::new(),
            bridges: BTreeMap::new(),
            next_idx: 1,
        }
    }

    fn alloc_idx(&mut self) -> u32 {
        let idx = self.next_idx;
        self.next_idx += 1;
        idx
    }
}

static CONTAINER_NETWORK: Mutex<Option<ContainerNetwork>> = Mutex::new(None);

pub fn init_container_network() {
    let mut net = CONTAINER_NETWORK.lock();
    *net = Some(ContainerNetwork::new());
}

pub fn reset_container_network() {
    let mut net = CONTAINER_NETWORK.lock();
    *net = None;
}

pub fn create_veth_pair(container_id: &str) -> Result<VethPair, NetworkError> {
    let mut guard = CONTAINER_NETWORK.lock();
    let net = guard
        .as_mut()
        .ok_or_else(|| NetworkError::InvalidConfig("network manager not initialized".into()))?;

    if net.veths.contains_key(container_id) {
        return Err(NetworkError::AlreadyExists);
    }

    let host_idx = net.alloc_idx();
    let container_idx = net.alloc_idx();

    let mut host_name = String::from("veth");
    push_u32(&mut host_name, host_idx);
    let mut container_name = String::from("veth");
    push_u32(&mut container_name, container_idx);

    let veth = VethPair {
        host_name: host_name.clone(),
        container_name,
        host_idx,
        container_idx,
    };

    net.veths
        .insert(String::from(container_id), veth.clone());

    Ok(veth)
}

pub fn delete_veth_pair(container_id: &str) -> bool {
    let mut guard = CONTAINER_NETWORK.lock();
    let net = match guard.as_mut() {
        Some(n) => n,
        None => return false,
    };
    net.veths.remove(container_id).is_some()
}

pub fn create_bridge(
    name: &str,
    ip: [u8; 4],
    netmask: [u8; 4],
) -> Result<(), NetworkError> {
    let mut guard = CONTAINER_NETWORK.lock();
    let net = guard
        .as_mut()
        .ok_or_else(|| NetworkError::InvalidConfig("network manager not initialized".into()))?;

    if net.bridges.contains_key(name) {
        return Err(NetworkError::AlreadyExists);
    }

    let bridge = Bridge {
        name: String::from(name),
        ip,
        netmask,
        interfaces: Vec::new(),
    };

    net.bridges.insert(String::from(name), bridge);
    Ok(())
}

pub fn attach_to_bridge(
    veth_host: &str,
    bridge_name: &str,
) -> Result<(), NetworkError> {
    let mut guard = CONTAINER_NETWORK.lock();
    let net = guard
        .as_mut()
        .ok_or_else(|| NetworkError::InvalidConfig("network manager not initialized".into()))?;

    let bridge = net
        .bridges
        .get_mut(bridge_name)
        .ok_or(NetworkError::NotFound)?;

    if bridge.interfaces.iter().any(|i| i == veth_host) {
        return Err(NetworkError::AlreadyExists);
    }

    bridge.interfaces.push(String::from(veth_host));
    Ok(())
}

pub fn get_container_veth(container_id: &str) -> Option<VethPair> {
    let guard = CONTAINER_NETWORK.lock();
    guard.as_ref()?.veths.get(container_id).cloned()
}

pub fn list_veths() -> Vec<(String, String)> {
    let guard = CONTAINER_NETWORK.lock();
    let net = match guard.as_ref() {
        Some(n) => n,
        None => return Vec::new(),
    };

    let mut result = Vec::new();
    for (_, veth) in &net.veths {
        result.push((veth.host_name.clone(), veth.container_name.clone()));
    }
    result
}

pub fn list_bridges() -> Vec<(String, [u8; 4])> {
    let guard = CONTAINER_NETWORK.lock();
    let net = match guard.as_ref() {
        Some(n) => n,
        None => return Vec::new(),
    };

    let mut result = Vec::new();
    for (_, bridge) in &net.bridges {
        result.push((bridge.name.clone(), bridge.ip));
    }
    result
}

fn push_u32(s: &mut String, val: u32) {
    if val == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    let mut v = val;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for &b in &buf[i..] {
        s.push(b as char);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() {
        init_container_network();
    }

    fn teardown() {
        reset_container_network();
    }

    #[test]
    fn init_and_reset_network() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert!(list_veths().is_empty());
        assert!(list_bridges().is_empty());
        teardown();
    }

    #[test]
    fn create_veth_pair_succeeds() {
        let _guard = crate::test_serial::acquire();
        setup();
        let veth = create_veth_pair("container1").unwrap();
        assert_eq!(veth.host_idx, 1);
        assert_eq!(veth.container_idx, 2);
        teardown();
    }

    #[test]
    fn create_veth_pair_duplicate_fails() {
        let _guard = crate::test_serial::acquire();
        setup();
        create_veth_pair("c1").unwrap();
        assert_eq!(create_veth_pair("c1"), Err(NetworkError::AlreadyExists));
        teardown();
    }

    #[test]
    fn delete_veth_pair_succeeds() {
        let _guard = crate::test_serial::acquire();
        setup();
        create_veth_pair("c1").unwrap();
        assert!(delete_veth_pair("c1"));
        assert!(get_container_veth("c1").is_none());
        teardown();
    }

    #[test]
    fn delete_veth_pair_not_found() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert!(!delete_veth_pair("nope"));
        teardown();
    }

    #[test]
    fn create_bridge_succeeds() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert!(create_bridge("br0", [10, 0, 0, 1], [255, 255, 255, 0]).is_ok());
        let bridges = list_bridges();
        assert_eq!(bridges.len(), 1);
        assert_eq!(bridges[0].0, "br0");
        teardown();
    }

    #[test]
    fn create_bridge_duplicate_fails() {
        let _guard = crate::test_serial::acquire();
        setup();
        create_bridge("br0", [10, 0, 0, 1], [255, 255, 255, 0]).unwrap();
        assert_eq!(
            create_bridge("br0", [10, 0, 0, 2], [255, 255, 255, 0]),
            Err(NetworkError::AlreadyExists)
        );
        teardown();
    }

    #[test]
    fn attach_to_bridge_succeeds() {
        let _guard = crate::test_serial::acquire();
        setup();
        create_bridge("br0", [10, 0, 0, 1], [255, 255, 255, 0]).unwrap();
        let veth = create_veth_pair("c1").unwrap();
        assert!(attach_to_bridge(&veth.host_name, "br0").is_ok());
        teardown();
    }

    #[test]
    fn attach_to_bridge_not_found() {
        let _guard = crate::test_serial::acquire();
        setup();
        assert_eq!(
            attach_to_bridge("veth1", "nobr"),
            Err(NetworkError::NotFound)
        );
        teardown();
    }

    #[test]
    fn list_veths_after_create() {
        let _guard = crate::test_serial::acquire();
        setup();
        create_veth_pair("c1").unwrap();
        create_veth_pair("c2").unwrap();
        let veths = list_veths();
        assert_eq!(veths.len(), 2);
        teardown();
    }
}
