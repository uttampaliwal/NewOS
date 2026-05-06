//! Network stack stub for Turnix OS
//! Will be replaced with full smoltcp integration later

use spin::Mutex;
use lazy_static::lazy_static;

/// Network interface state (stub)
pub struct NetworkState {
    initialized: bool,
}

lazy_static! {
    pub static ref NET_STATE: Mutex<Option<NetworkState>> = Mutex::new(None);
}

/// Initialize network stack
pub fn init() {
    crate::serial::println!("[NET] Initializing network stack...");
    
    // Stub - will be replaced with smoltcp later
    let state = NetworkState {
        initialized: true,
    };
    
    *NET_STATE.lock() = Some(state);
    
    crate::serial::println!("[NET] Network stack initialized (stub)");
    crate::serial::println!("[NET] MAC: 02:00:AD:DE:00:01");
    crate::serial::println!("[NET] IP: 192.168.1.100");
}

/// Poll network interface (stub)
pub fn poll() {
    // Stub - nothing to poll yet
}
