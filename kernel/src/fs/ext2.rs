//! ext2 filesystem driver for Turnix OS
//! Provides persistent storage via ext2 filesystem.

extern crate alloc;

use spin::Mutex;
use alloc::vec::Vec;
use alloc::vec;
use core::ptr;
use alloc::string::String;

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
    pub reserved: [u8; 768],
}

impl Ext2Superblock {
    pub fn is_valid(&self) -> bool {
        let magic_ptr = ptr::addr_of!(self.magic);
        let magic = unsafe { ptr::read_unaligned(magic_ptr) };
        magic == 0xEF53
    }

    pub fn block_size(&self) -> usize {
        let log_bs_ptr = ptr::addr_of!(self.log_block_size);
        let log_bs = unsafe { ptr::read_unaligned(log_bs_ptr) };
        1024 << log_bs
    }

    pub fn get_inode_count(&self) -> u32 {
        let ptr = ptr::addr_of!(self.inode_count);
        unsafe { ptr::read_unaligned(ptr) }
    }

    pub fn get_block_count(&self) -> u32 {
        let ptr = ptr::addr_of!(self.block_count);
        unsafe { ptr::read_unaligned(ptr) }
    }

    pub fn get_inodes_per_group(&self) -> u32 {
        let ptr = ptr::addr_of!(self.inodes_per_group);
        unsafe { ptr::read_unaligned(ptr) }
    }

    pub fn get_blocks_per_group(&self) -> u32 {
        let ptr = ptr::addr_of!(self.blocks_per_group);
        unsafe { ptr::read_unaligned(ptr) }
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
        let mode_ptr = ptr::addr_of!(self.mode);
        let mode = unsafe { ptr::read_unaligned(mode_ptr) };
        mode & 0x4000 != 0
    }

    pub fn is_regular_file(&self) -> bool {
        let mode_ptr = ptr::addr_of!(self.mode);
        let mode = unsafe { ptr::read_unaligned(mode_ptr) };
        mode & 0x8000 != 0
    }

    pub fn size(&self) -> u64 {
        let size_ptr = ptr::addr_of!(self.size_low);
        let size_low = unsafe { ptr::read_unaligned(size_ptr) };
        size_low as u64
    }

    pub fn get_block(&self, idx: usize) -> u32 {
        if idx >= 15 { return 0; }
        let block_ptr = unsafe { (self as *const _ as *const u8).add(40 + idx * 4) as *const u32 };
        unsafe { ptr::read_unaligned(block_ptr) }
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

impl Ext2DirEntry {
    pub fn get_inode(&self) -> u32 {
        let ptr = ptr::addr_of!(self.inode);
        unsafe { ptr::read_unaligned(ptr) }
    }

    pub fn get_rec_len(&self) -> u16 {
        let ptr = ptr::addr_of!(self.rec_len);
        unsafe { ptr::read_unaligned(ptr) }
    }

    pub fn get_name_len(&self) -> u8 {
        let ptr = ptr::addr_of!(self.name_len);
        unsafe { ptr::read_unaligned(ptr) }
    }

    pub fn get_file_type(&self) -> u8 {
        let ptr = ptr::addr_of!(self.file_type);
        unsafe { ptr::read_unaligned(ptr) }
    }
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

impl GroupDesc {
    pub fn get_inode_table(&self) -> u32 {
        let ptr = ptr::addr_of!(self.inode_table);
        unsafe { ptr::read_unaligned(ptr) }
    }
}

/// ext2 filesystem state
pub struct Ext2Fs {
    pub superblock: Option<Ext2Superblock>,
    pub block_size: usize,
    pub device_id: usize,
    pub group_descs: Vec<GroupDesc>,
}

/// Global ext2 state
static EXT2_FS: Mutex<Option<Ext2Fs>> = Mutex::new(None);

/// Read blocks from block device (using AHCI)
fn read_blocks(device_id: usize, lba: u64, count: usize, buffer: &mut [u8]) -> bool {
    crate::serial::println!("[EXT2] Reading {} blocks from LBA {} on device {}", count, lba, device_id);
    crate::drivers::ahci::read_blocks(device_id, lba, count, buffer)
}

/// Read superblock from device
fn read_superblock(device_id: usize) -> Option<Ext2Superblock> {
    let block_size = 1024usize;
    let mut buffer = vec![0u8; block_size];

    // Superblock is at offset 1024, which is sector 2 for 512-byte sectors
    if !read_blocks(device_id, 2, 2, &mut buffer) {
        crate::serial::println!("[EXT2] Failed to read superblock from device");
        return None;
    }

    // Read superblock from buffer
    let sb = unsafe { ptr::read_unaligned(buffer.as_ptr() as *const Ext2Superblock) };

    if !sb.is_valid() {
        crate::serial::println!("[EXT2] Invalid superblock");
        return None;
    }

    crate::serial::println!("[EXT2] Valid superblock: block_size={}", sb.block_size());
    Some(sb)
}

/// Initialize ext2 filesystem on a block device.
pub fn init(device_id: usize) -> bool {
    crate::serial::println!("[EXT2] Initializing ext2 on device {}", device_id);

    let superblock = match read_superblock(device_id) {
        Some(sb) => sb,
        None => {
            crate::serial::println!("[EXT2] Failed to read superblock");
            return false;
        }
    };

    let block_size = superblock.block_size();
    let block_count = superblock.get_block_count();
    let _inodes_per_group = superblock.get_inodes_per_group();
    let group_count = ((block_count + superblock.get_blocks_per_group() - 1) / superblock.get_blocks_per_group()) as usize;

    crate::serial::println!("[EXT2] Filesystem: {} blocks, {} inodes, {} groups",
        block_size, superblock.get_inode_count(), group_count);

    // Read group descriptors (located after superblock)
    let gd_block = if block_size == 1024 { 2u64 } else { 1u64 };
    let gd_size = group_count * 32; // Each group desc is 32 bytes
    let mut gd_buffer = vec![0u8; ((gd_size + block_size - 1) / block_size) * block_size];

    if !read_blocks(device_id, gd_block, gd_buffer.len() / 512, &mut gd_buffer) {
        crate::serial::println!("[EXT2] Failed to read group descriptors");
    }

    let mut group_descs = Vec::new();
    for i in 0..group_count {
        if i * 32 + 32 <= gd_buffer.len() {
            let gd = unsafe { ptr::read_unaligned(gd_buffer.as_ptr().add(i * 32) as *const GroupDesc) };
            group_descs.push(gd);
        }
    }

    crate::serial::println!("[EXT2] Read {} group descriptors", group_descs.len());

    let fs = Ext2Fs {
        superblock: Some(superblock),
        block_size,
        device_id,
        group_descs,
    };

    *EXT2_FS.lock() = Some(fs);

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
pub fn read_inode(device_id: usize, ino: u32) -> Option<Ext2Inode> {
    let guard = EXT2_FS.lock();
    if let Some(ref fs) = *guard {
        if let Some(ref sb) = fs.superblock {
            let block_size = fs.block_size;
            let inodes_per_group = sb.get_inodes_per_group();
            let inode_size = 128usize; // Standard ext2 inode size

            let group = (ino - 1) / inodes_per_group;
            let index = (ino - 1) % inodes_per_group;

            if (group as usize) < fs.group_descs.len() {
                let gd = &fs.group_descs[group as usize];
                let inode_table = gd.get_inode_table();
                let inode_table_lba = inode_table as u64 * (block_size / 512) as u64;
                let inode_offset = index as u64 * inode_size as u64;

                let mut buffer = vec![0u8; block_size];
                if read_blocks(device_id, inode_table_lba, block_size / 512, &mut buffer) {
                    let inode_ptr = unsafe { buffer.as_ptr().add(inode_offset as usize) as *const Ext2Inode };
                    let inode = unsafe { ptr::read_unaligned(inode_ptr) };
                    return Some(inode);
                }
            }
        }
    }
    None
}

/// List directory contents
pub fn list_dir(device_id: usize, ino: u32) -> Vec<(u32, String, u8)> {
    let mut result = Vec::new();

    let block_size = {
        let guard = EXT2_FS.lock();
        guard.as_ref().map(|fs| fs.block_size).unwrap_or(1024)
    };

    if let Some(inode) = read_inode(device_id, ino) {
        let size = inode.size() as usize;

        // Read direct blocks (simplified - only first 12 blocks)
        for i in 0..12 {
            let block_num = inode.get_block(i);
            if block_num == 0 {
                break;
            }

            let mut buffer = vec![0u8; block_size];
            let lba = block_num as u64 * (block_size / 512) as u64;
            if !read_blocks(device_id, lba, block_size / 512, &mut buffer) {
                break;
            }

            // Parse directory entries
            let mut offset = 0;
            while offset < buffer.len() && offset < size {
                let entry_ptr = unsafe { buffer.as_ptr().add(offset) as *const Ext2DirEntry };
                let entry = unsafe { ptr::read_unaligned(entry_ptr) };

                let rec_len = entry.get_rec_len() as usize;
                let name_len = entry.get_name_len() as usize;
                let inode_num = entry.get_inode();
                let file_type = entry.get_file_type();

                if inode_num == 0 || rec_len == 0 {
                    break;
                }

                if name_len > 0 && name_len <= 255 {
                    let name_cow = String::from_utf8_lossy(&buffer[offset + 8..offset + 8 + name_len]);
                    let name = String::from(&*name_cow); // Convert Cow<str> to String
                    result.push((inode_num, name, file_type));
                }

                offset += rec_len;
            }
        }
    }

    result
}
