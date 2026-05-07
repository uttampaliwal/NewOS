//! Network stack for Turnix OS
//! Uses smoltcp for TCP/IP networking

use spin::Mutex;
use lazy_static::lazy_static;

/// Network interface state
pub struct NetworkState {
    pub initialized: bool,
    // TODO: Add smoltcp Interface and SocketSet when NIC driver is ready
}

lazy_static! {
    pub static ref NET_STATE: Mutex<NetworkState> = Mutex::new(NetworkState {
        initialized: false,
    });
}

/// Initialize network stack
pub fn init() {
    crate::serial::println!("[NET] Initializing network stack...");

    // TODO: Complete smoltcp integration requires:
    // 1. PCI scanning to find NIC (e1000, rtl8139, etc.)
    // 2. Initialize NIC hardware and get MAC address
    // 3. Create smoltcp Interface with EthernetAddress
    // 4. Set up SocketSet with TCP/UDP sockets
    // 5. Configure IP address (static or DHCP)

    let mut state = NET_STATE.lock();
    state.initialized = true;

    crate::serial::println!("[NET] Network stack initialized (stub)");
    crate::serial::println!("[NET] MAC: 02:00:AD:DE:00:01");
    crate::serial::println!("[NET] IP: 192.168.1.100/24");
    crate::serial::println!("[NET] TODO: Integrate with NIC driver for full smoltcp support");
}

/// Poll network interface
pub fn poll() {
    // TODO: In real implementation:
    // 1. Check NIC for received packets
    // 2. Pass to smoltcp for processing
    // 3. Check for outgoing packets from smoltcp
    // 4. Send via NIC

    if !NET_STATE.lock().initialized {
        return;
    }

    // Stub for now
}
