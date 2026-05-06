//! Filesystem module for Turnix OS
//! Supports persistent filesystems like ext2

pub mod ext2;

use lazy_static::lazy_static;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// Filesystem types supported
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FsType {
    Ext2,
    Fat32,
    Unknown,
}

/// Filesystem mount information
pub struct Mount {
    pub device: usize,  // Block device number
    pub fs_type: FsType,
    pub mount_point: String,
    pub root_inode: u32,  // Root inode number (for ext2)
}

lazy_static! {
    pub static ref MOUNTS: Mutex<Vec<Mount>> = Mutex::new(Vec::new());
}

/// Initialize filesystem subsystem
pub fn init() {
    crate::serial::println!("[FS] Initializing filesystem subsystem...");
    // For now, just print that we're ready
    crate::serial::println!("[FS] Filesystem subsystem initialized");
}

/// Mount a filesystem
pub fn mount(device: usize, fs_type: FsType, mount_point: &str) -> bool {
    crate::serial::println!(
        "[FS] Mounting {:?} filesystem from device {} at {}",
        fs_type, device, mount_point
    );
    
    let mount = Mount {
        device,
        fs_type,
        mount_point: String::from(mount_point),
        root_inode: 2, // ext2 root inode is usually 2
    };
    
    MOUNTS.lock().push(mount);
    crate::serial::println!("[FS] Mount complete");
    true
}
