pub mod ahci;
pub mod framework;
pub mod net;
pub mod pci;
pub mod pcie;
pub mod video;

use lazy_static::lazy_static;
use spin::Mutex;

use framework::DeviceRegistry;

lazy_static! {
    pub static ref DEVICE_REGISTRY: Mutex<DeviceRegistry> = Mutex::new(DeviceRegistry::new());
}