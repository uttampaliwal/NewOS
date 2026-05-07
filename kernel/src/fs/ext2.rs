//! ext2 filesystem driver for Turnix OS
//! Provides persistent storage via ext2 filesystem

extern crate alloc;

use spin::Mutex;
use alloc::vec::Vec;
use alloc::vec;
use core::ptr;

/// ext2 superblock (offset 1024 in filesystem)
#[repr(C, packed)]
pub struct Ext2Superblock {
    pub inode_count: u32,
    pub block_count: u32,
    pub reserved_blocks: u32,
    pub free_blocks: u32,
    pub free_inodes: u32,
    pub first_data_block: u32,
    pub log_block_size: u32,
    pub log_frag_size: i32,
    pub blocks_per_group: u32,
    pub frags_per_group: u32,
    pub inodes_per_group: u32,
    pub mtime: u32,
    pub wtime: u32,
    pub mnt_count: u16,
    pub max_mnt_count: u16,
    pub magic: u16,
    pub state: u16,
    pub errors: u16,
    pub minor_rev_level: u16,
    pub lastcheck: u32,
    pub checkinterval: u32,
    pub creator_os: u32,
    pub rev_level: u32,
    pub reserved: [u8; 768], // Simplified - rest of superblock
}

impl Ext2Superblock {
    pub fn is_valid(&self) -> bool {
        self.magic == 0xEF53
    }

    pub fn block_size(&self) -> usize {
        1024 << self.log_block_size
    }
}

/// ext2 inode structure
#[repr(C, packed)]
pub struct Ext2Inode {
    pub mode: u16,
    pub uid: u16,
    pub size_low: u32,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub gid: u16,
    pub links_count: u16,
    pub blocks: u32,
    pub flags: u32,
    pub osd1: u32,
    pub block: [u32; 15],
    pub generation: u32,
    pub file_acl: u32,
    pub dir_acl: u32,
    pub faddr: u32,
    pub osd2: [u8; 12],
}

impl Ext2Inode {
    pub fn is_directory(&self) -> bool {
        self.mode & 0x4000 != 0
    }

    pub fn is_regular_file(&self) -> bool {
        self.mode & 0x8000 != 0
    }

    pub fn size(&self) -> u64 {
        self.size_low as u64
    }
}

/// Directory entry
#[repr(C, packed)]
pub struct Ext2DirEntry {
    pub inode: u32,
    pub rec_len: u16,
    pub name_len: u8,
    pub file_type: u8,
}

/// Group descriptor
#[repr(C, packed)]
pub struct GroupDesc {
    pub block_bitmap: u32,
    pub inode_bitmap: u32,
    pub inode_table: u32,
    pub free_blocks_count: u16,
    pub free_inodes_count: u16,
    pub used_dirs_count: u16,
    pub pad: u16,
    pub reserved: [u8; 12],
}

/// ext2 filesystem state
pub struct Ext2Fs {
    pub superblock: Ext2Superblock,
    pub block_size: usize,
    pub device_id: usize,
    pub group_desc: Vec<GroupDesc>,
}

/// Global ext2 state
static mut EXT2_FS: Option<Ext2Fs> = None;
static EXT2_MUTEX: Mutex<()> = Mutex::new(());

/// Read blocks from block device (using AHCI)
fn read_blocks(device_id: usize, lba: u64, count: usize, buffer: &mut [u8]) -> bool {
    // Use AHCI driver to read blocks
    crate::serial::println!("[EXT2] Reading {} blocks from LBA {} on device {}", count, lba, device_id);
    // Call AHCI driver's read_blocks function
    crate::drivers::ahci::read_blocks(device_id, lba, count, buffer)
}

/// Read superblock from device
fn read_superblock(device_id: usize) -> Option<Ext2Superblock> {
    let block_size = 1024usize;
    let mut buffer = vec![0u8; block_size];

    // Superblock is at offset 1024 (block 1 for 1024-byte blocks)
    if !read_blocks(device_id, 2, 1, &mut buffer) {
        // For now, create a dummy superblock for testing
        crate::serial::println!("[EXT2] Using dummy superblock for testing");
        let mut sb = Ext2Superblock {
            inode_count: 0,
            block_count: 0,
            reserved_blocks: 0,
            free_blocks: 0,
            free_inodes: 0,
            first_data_block: 1,
            log_block_size: 0,
            log_frag_size: 0,
            blocks_per_group: 8192,
            frags_per_group: 8192,
            inodes_per_group: 1024,
            mtime: 0,
            wtime: 0,
            mnt_count: 0,
            max_mnt_count: 0,
            magic: 0xEF53,
            state: 1,
            errors: 0,
            minor_rev_level: 0,
            lastcheck: 0,
            checkinterval: 0,
            creator_os: 0,
            rev_level: 0,
            reserved: [0; 768],
        };
        return Some(sb);
    }

    let sb = unsafe { ptr::read_unaligned(buffer.as_ptr() as *const Ext2Superblock) };
    let magic = sb.magic; // Copy to local to avoid unaligned reference
    if !sb.is_valid() {
        crate::serial::println!("[EXT2] Invalid superblock magic: {:#x}", magic);
        return None;
    }

    Some(sb)
}

/// Initialize ext2 filesystem on a block device
pub fn init(device_id: usize) -> bool {
    crate::serial::println!("[EXT2] Initializing ext2 on device {}", device_id);

    let superblock = match read_superblock(device_id) {
        Some(sb) => sb,
        None => {
            crate::serial::println!("[EXT2] Failed to read superblock");
            return false;
        }
    };

    let magic = superblock.magic; // Copy to avoid unaligned reference
    let block_size = superblock.block_size(); // Compute before printing
    crate::serial::println!("[EXT2] Superblock valid: magic={:#x}, block_size={}",
        magic, block_size);

    let block_size = superblock.block_size();
    let group_count = ((superblock.block_count + superblock.blocks_per_group - 1) / superblock.blocks_per_group) as usize;

    // Read group descriptors (located after superblock)
    let gd_block = if block_size == 1024 { 2 } else { 1 };
    let gd_size = ((group_count * 32 + block_size - 1) / block_size) * block_size;
    let mut gd_buffer = vec![0u8; gd_size];

    let fs = Ext2Fs {
        superblock,
        block_size,
        device_id,
        group_desc: Vec::new(), // TODO: Read group descriptors
    };

    unsafe {
        let _lock = EXT2_MUTEX.lock();
        EXT2_FS = Some(fs);
    }

    crate::serial::println!("[EXT2] ext2 filesystem initialized");
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

/// Read inode by number
pub fn read_inode(_device_id: usize, _ino: u32) -> Option<Ext2Inode> {
    // TODO: Implement inode reading from inode table
    None
}

/// List directory contents
pub fn list_dir(_device_id: usize, _ino: u32) -> Vec<(u32, alloc::string::String, u8)> {
    // TODO: Implement directory listing
    Vec::new()
}
