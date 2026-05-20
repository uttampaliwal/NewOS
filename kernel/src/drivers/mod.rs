pub mod ahci;
pub mod framework;
pub mod net;
pub mod nvme;
pub mod pci;
pub mod pcie;
pub mod video;
pub mod virtio_net;

use lazy_static::lazy_static;
use spin::Mutex;

use framework::DeviceRegistry;

lazy_static! {
    pub static ref DEVICE_REGISTRY: Mutex<DeviceRegistry> = Mutex::new(DeviceRegistry::new());
}