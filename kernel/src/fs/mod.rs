//! Filesystem module for Turnix OS
//! Supports persistent filesystems like ext2 and the new VFS layer.

pub mod ext2;
pub mod ext4;
pub mod tmpfs;
pub mod vfs;

use alloc::string::String;
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::Mutex;

/// Filesystem types supported
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FsType {
    Ext2,
    Ext4,
    Fat32,
    Unknown,
}

/// Filesystem mount information
pub struct Mount {
    pub device: usize, // Block device number
    pub fs_type: FsType,
    pub mount_point: String,
    pub root_inode: u32, // Root inode number (for ext2)
}

lazy_static! {
    pub static ref MOUNTS: Mutex<Vec<Mount>> = Mutex::new(Vec::new());
}

/// Initialize filesystem subsystem
///
/// Mounts the root tmpfs and populates the initial directory layout.
pub fn init() {
    crate::serial::println!("[FS] Initializing filesystem subsystem...");

    // Mount root tmpfs at "/"
    {
        let mut vfs = crate::vfs::VFS.lock();
        let backend = alloc::sync::Arc::new(tmpfs::TmpfsBackend::new());
        if vfs.mount(
            "/",
            backend,
            crate::vfs::MountFlags::default(),
        ).is_ok() {
            crate::serial::println!("[FS] Root tmpfs mounted at /");
        }

        // Create standard directories
        vfs.mkdir("/dev");
        vfs.mkdir("/proc");
        vfs.mkdir("/sys");
        vfs.mkdir("/tmp");
        vfs.mkdir("/var");
        vfs.mkdir("/etc");
        vfs.mkdir("/home");
        vfs.mkdir("/root");
        vfs.mkdir("/usr");
        vfs.mkdir("/mnt");
    }

    crate::serial::println!("[FS] Filesystem subsystem initialized");
}

/// Mount a filesystem at the given mount point.
///
/// Creates the appropriate backend based on `fs_type` and registers it
/// with the VFS layer.
pub fn mount(device: usize, fs_type: FsType, mount_point: &str) -> bool {
    crate::serial::println!(
        "[FS] Mounting {:?} filesystem from device {} at {}",
        fs_type,
        device,
        mount_point
    );

    // All current filesystem types return early (not yet implemented).
    // Once block device drivers are available, Ext2/Ext4 backends will be
    // created here and the VFS mount will be called with a real backend.
    match fs_type {
        FsType::Ext2 | FsType::Ext4 => {
            crate::serial::println!("[FS] Ext4 backend not yet available for device {}", device);
            false
        }
        FsType::Fat32 => {
            crate::serial::println!("[FS] Fat32 backend not yet available for device {}", device);
            false
        }
        FsType::Unknown => {
            crate::serial::println!("[FS] Unknown filesystem type for device {}", device);
            false
        }
    }
}
