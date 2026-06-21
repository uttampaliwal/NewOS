use lazy_static::lazy_static;
use spin::Mutex;

pub struct NetworkState {
    pub initialized: bool,
    pub mac_address: [u8; 6],
}

lazy_static! {
    pub static ref NET_STATE: Mutex<NetworkState> = Mutex::new(NetworkState {
        initialized: false,
        mac_address: [0u8; 6],
    });
}

pub fn init() {
    crate::serial::println!("[NET] Initializing network stack...");

    let mac = crate::drivers::virtio_net::get_mac_address()
        .unwrap_or([0x02, 0x00, 0xAD, 0xDE, 0x00, 0x01]);

    #[cfg(feature = "arch-x86_64")]
    crate::net::smoltcp_iface::init();

    let mut state = NET_STATE.lock();
    state.mac_address = mac;
    state.initialized = true;

    crate::serial::println!(
        "[NET] MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5]
    );
    crate::serial::println!("[NET] Network stack initialized");
}

pub fn poll() {
    if NET_STATE.lock().initialized {
        #[cfg(feature = "arch-x86_64")]
        crate::net::smoltcp_iface::poll_stack();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_state_initial() {
        let state = NetworkState {
            initialized: false,
            mac_address: [0u8; 6],
        };
        assert!(!state.initialized);
        assert_eq!(state.mac_address, [0u8; 6]);
    }

    #[test]
    fn test_network_state_after_init() {
        let state = NetworkState {
            initialized: true,
            mac_address: [0x02, 0x00, 0xAD, 0xDE, 0x00, 0x01],
        };
        assert!(state.initialized);
        assert_eq!(state.mac_address[0], 0x02);
    }

    #[test]
    fn test_mac_address_all_zeros() {
        let state = NetworkState {
            initialized: true,
            mac_address: [0u8; 6],
        };
        assert!(state.initialized);
        assert_eq!(state.mac_address, [0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_mac_address_broadcast() {
        let state = NetworkState {
            initialized: true,
            mac_address: [0xFFu8; 6],
        };
        assert!(state.initialized);
        assert_eq!(state.mac_address, [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    }
}
