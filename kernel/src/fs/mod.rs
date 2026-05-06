//! Filesystem module for Turnix OS
//! Supports persistent filesystems like ext2

pub mod ext2;

/// Initialize filesystem subsystem
pub fn init() {
    crate::serial::println!("[FS] Initializing filesystem subsystem...");
    crate::serial::println!("[FS] Filesystem subsystem initialized");
}
