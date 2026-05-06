//! ext2 filesystem driver for Turnix OS
//! Provides persistent storage via ext2 filesystem

use spin::Mutex;

/// ext2 superblock (offset 1024 in filesystem)
#[repr(C, packed)]
pub struct Ext2Superblock {
    pub inode_count: u32,       // Total number of inodes
    pub block_count: u32,      // Total number of blocks
    pub reserved_blocks: u32,   // Number of reserved blocks
    pub free_blocks: u32,      // Number of free blocks
    pub free_inodes: u32,     // Number of free inodes
    pub first_data_block: u32, // First data block (usually 1)
    pub log_block_size: u32,   // log2(block size) - 10 (block size = 1024 << log_block_size)
    pub log_frag_size: i32,    // log2(fragment size)
    pub blocks_per_group: u32,  // Number of blocks per group
    pub frags_per_group: u32,   // Number of fragments per group
    pub inodes_per_group: u32,  // Number of inodes per group
    pub magic: u16,              // Magic number (0xEF53)
    pub state: u16,              // Filesystem state (1 = clean, 2 = has errors)
}

impl Ext2Superblock {
    /// Validate the superblock
    pub fn is_valid(&self) -> bool {
        self.magic == 0xEF53
    }
    
    /// Get block size in bytes
    pub fn block_size(&self) -> usize {
        1024 << self.log_block_size
    }
}

/// ext2 filesystem state
pub struct Ext2Fs {
    pub superblock: Option<Ext2Superblock>,
    pub block_size: usize,
    pub device_id: usize,
}

/// Global ext2 state (stub for now)
static mut EXT2_FS_STATE: Option<Ext2Fs> = None;
static EXT2_MUTEX: Mutex<()> = Mutex::new(());

/// Initialize ext2 filesystem on a block device
pub fn init(device_id: usize) -> bool {
    crate::serial::println!("[EXT2] Initializing ext2 on device {}", device_id);
    
    // Stub - would read superblock from disk using AHCI driver
    let fs = Ext2Fs {
        superblock: None,
        block_size: 1024,
        device_id,
    };
    
    unsafe {
        let _lock = EXT2_MUTEX.lock();
        EXT2_FS_STATE = Some(fs);
    }
    
    crate::serial::println!("[EXT2] ext2 filesystem initialized (stub)");
    crate::serial::println!("[EXT2] TODO: Read superblock from block device");
    
    true
}

/// Mount ext2 filesystem
pub fn mount(device_id: usize) -> bool {
    crate::serial::println!("[EXT2] Mounting ext2 filesystem from device {}", device_id);
    
    if !init(device_id) {
        return false;
    }
    
    // Register with VFS
    crate::fs::mount(device_id, crate::fs::FsType::Ext2, "/");
    
    true
}
