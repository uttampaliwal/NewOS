use spin::Mutex;
use lazy_static::lazy_static;

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

    let mac = crate::drivers::virtio_net::get_mac_address().unwrap_or([0x02, 0x00, 0xAD, 0xDE, 0x00, 0x01]);

    let mut state = NET_STATE.lock();
    state.mac_address = mac;
    state.initialized = true;

    crate::serial::println!(
        "[NET] MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    );
    crate::serial::println!("[NET] Network stack initialized");
}

pub fn poll() {
    if !NET_STATE.lock().initialized {
        return;
    }
}
