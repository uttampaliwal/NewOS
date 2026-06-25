//! ext2 filesystem driver for Turnix OS
//! Provides persistent storage via ext2 filesystem.

pub mod allocator;
pub mod disk;
pub mod write;

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ptr;
use spin::Mutex;

use crate::fs::vfs::{DirEntry, FileType, FsBackend, FsError, InodeId, InodeStat, OpenFlags};

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
        // Safety: self is a valid packed struct; magic_ptr points within it.
        let magic = unsafe { ptr::read_unaligned(magic_ptr) };
        magic == 0xEF53
    }

    pub fn block_size(&self) -> usize {
        let log_bs_ptr = ptr::addr_of!(self.log_block_size);
        // Safety: self is a valid packed struct; log_bs_ptr points within it.
        let log_bs = unsafe { ptr::read_unaligned(log_bs_ptr) };
        1024 << log_bs
    }

    pub fn get_inode_count(&self) -> u32 {
        let ptr = ptr::addr_of!(self.inode_count);
        // Safety: self is a valid packed struct; ptr points within it.
        unsafe { ptr::read_unaligned(ptr) }
    }

    pub fn get_block_count(&self) -> u32 {
        let ptr = ptr::addr_of!(self.block_count);
        // Safety: self is a valid packed struct; ptr points within it.
        unsafe { ptr::read_unaligned(ptr) }
    }

    pub fn get_inodes_per_group(&self) -> u32 {
        let ptr = ptr::addr_of!(self.inodes_per_group);
        // Safety: self is a valid packed struct; ptr points within it.
        unsafe { ptr::read_unaligned(ptr) }
    }

    pub fn get_blocks_per_group(&self) -> u32 {
        let ptr = ptr::addr_of!(self.blocks_per_group);
        // Safety: self is a valid packed struct; ptr points within it.
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
        // Safety: self is a valid packed struct; mode_ptr points within it.
        let mode = unsafe { ptr::read_unaligned(mode_ptr) };
        mode & 0x4000 != 0
    }

    pub fn is_regular_file(&self) -> bool {
        let mode_ptr = ptr::addr_of!(self.mode);
        // Safety: self is a valid packed struct; mode_ptr points within it.
        let mode = unsafe { ptr::read_unaligned(mode_ptr) };
        mode & 0x8000 != 0
    }

    pub fn size(&self) -> u64 {
        let size_ptr = ptr::addr_of!(self.size_low);
        // Safety: self is a valid packed struct; size_ptr points within it.
        let size_low = unsafe { ptr::read_unaligned(size_ptr) };
        size_low as u64
    }

    pub fn get_block(&self, idx: usize) -> u32 {
        if idx >= 15 {
            return 0;
        }
        // Safety: self is a valid packed struct; the offset 40 + idx*4 is within the block[15] field.
        let block_ptr = unsafe { (self as *const _ as *const u8).add(40 + idx * 4) as *const u32 };
        // Safety: block_ptr points to a valid u32 within the packed struct's block array.
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
        // Safety: self is a valid packed struct; ptr points within it.
        unsafe { ptr::read_unaligned(ptr) }
    }

    pub fn get_rec_len(&self) -> u16 {
        let ptr = ptr::addr_of!(self.rec_len);
        // Safety: self is a valid packed struct; ptr points within it.
        unsafe { ptr::read_unaligned(ptr) }
    }

    pub fn get_name_len(&self) -> u8 {
        let ptr = ptr::addr_of!(self.name_len);
        // Safety: self is a valid packed struct; ptr points within it.
        unsafe { ptr::read_unaligned(ptr) }
    }

    pub fn get_file_type(&self) -> u8 {
        let ptr = ptr::addr_of!(self.file_type);
        // Safety: self is a valid packed struct; ptr points within it.
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
        // Safety: self is a valid packed struct; ptr points within it.
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
    crate::serial::println!(
        "[EXT2] Reading {} blocks from LBA {} on device {}",
        count,
        lba,
        device_id
    );
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

    // Safety: buffer contains a valid superblock read from disk at the correct offset.
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
    let group_count = block_count.div_ceil(superblock.get_blocks_per_group()) as usize;

    crate::serial::println!(
        "[EXT2] Filesystem: {} blocks, {} inodes, {} groups",
        block_size,
        superblock.get_inode_count(),
        group_count
    );

    // Read group descriptors (located after superblock)
    let gd_block = if block_size == 1024 { 2u64 } else { 1u64 };
    let gd_size = group_count * 32; // Each group desc is 32 bytes
    let mut gd_buffer = vec![0u8; gd_size.div_ceil(block_size) * block_size];

    if !read_blocks(device_id, gd_block, gd_buffer.len() / 512, &mut gd_buffer) {
        crate::serial::println!("[EXT2] Failed to read group descriptors");
    }

    let mut group_descs = Vec::new();
    for i in 0..group_count {
        if i * 32 + 32 <= gd_buffer.len() {
            // Safety: gd_buffer contains valid group descriptor data read from disk; offset i*32 is within bounds.
            let gd =
                unsafe { ptr::read_unaligned(gd_buffer.as_ptr().add(i * 32) as *const GroupDesc) };
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
    let fs = guard.as_ref()?;
    let sb = fs.superblock.as_ref()?;
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
            // Safety: buffer contains valid inode table data read from disk; inode_offset is within bounds.
            let inode_ptr =
                unsafe { buffer.as_ptr().add(inode_offset as usize) as *const Ext2Inode };
            // Safety: inode_ptr points to a valid Ext4Inode within the buffer.
            let inode = unsafe { ptr::read_unaligned(inode_ptr) };
            return Some(inode);
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
                // Safety: buffer contains valid directory data; offset < buffer.len() and < size.
                let entry_ptr = unsafe { buffer.as_ptr().add(offset) as *const Ext2DirEntry };
                // Safety: entry_ptr points to a valid Ext2DirEntry within the buffer.
                let entry = unsafe { ptr::read_unaligned(entry_ptr) };

                let rec_len = entry.get_rec_len() as usize;
                let name_len = entry.get_name_len() as usize;
                let inode_num = entry.get_inode();
                let file_type = entry.get_file_type();

                if inode_num == 0 || rec_len == 0 {
                    break;
                }

                if name_len > 0 && name_len <= 255 {
                    let name_cow =
                        String::from_utf8_lossy(&buffer[offset + 8..offset + 8 + name_len]);
                    let name = String::from(&*name_cow);
                    result.push((inode_num, name, file_type));
                }

                offset += rec_len;
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Ext2Backend — FsBackend adapter (read-only)
// ---------------------------------------------------------------------------

/// Read-only [`FsBackend`] adapter for the ext2 filesystem.
///
/// Wraps the raw ext2 reader functions (`read_inode`, `list_dir`) and exposes
/// them through the VFS [`FsBackend`] trait.
///
/// All write operations (`write`, `mkdir`, `unlink`, `rename`) return
/// [`FsError::PermissionDenied`] because ext2 is mounted read-only.
pub struct Ext2Backend {
    /// The AHCI/SATA device index to read blocks from.
    pub device_id: usize,
}

impl Ext2Backend {
    /// Create a new `Ext2Backend` for the given device.
    ///
    /// Callers should call [`init`] on the device before constructing this.
    pub fn new(device_id: usize) -> Self {
        Ext2Backend { device_id }
    }
}

/// Convert ext2 `mode` bits to a [`FileType`].
fn mode_to_file_type(mode: u16) -> FileType {
    let fmt = mode & 0xF000;
    match fmt {
        0x4000 => FileType::Directory,
        0x8000 => FileType::Regular,
        0x2000 => FileType::Device, // char device
        0x6000 => FileType::Device, // block device
        0x1000 => FileType::Pipe,
        0xC000 => FileType::Socket,
        0xA000 => FileType::Symlink,
        _ => FileType::Regular,
    }
}

/// Convert ext2 dir-entry file-type byte to [`FileType`].
fn dir_ftype_to_file_type(ft: u8) -> FileType {
    match ft {
        1 => FileType::Regular,
        2 => FileType::Directory,
        3 => FileType::Device, // char device
        4 => FileType::Device, // block device
        5 => FileType::Pipe,
        6 => FileType::Socket,
        7 => FileType::Symlink,
        _ => FileType::Regular,
    }
}

impl FsBackend for Ext2Backend {
    fn root_inode(&self) -> InodeId {
        // ext2 root is always inode 2.
        InodeId(2)
    }

    fn lookup(&self, parent: InodeId, name: &str) -> Result<InodeId, FsError> {
        let entries = list_dir(self.device_id, parent.0 as u32);
        for (ino, entry_name, _ft) in entries {
            if entry_name == name {
                return Ok(InodeId(ino as u64));
            }
        }
        Err(FsError::NotFound)
    }

    fn open(&self, _inode: InodeId, flags: OpenFlags) -> Result<(), FsError> {
        if flags.writable() {
            return Err(FsError::PermissionDenied);
        }
        Ok(())
    }

    fn read(&self, inode: InodeId, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let ino = inode.0 as u32;
        let ext2_inode = read_inode(self.device_id, ino).ok_or(FsError::IoError)?;
        let file_size = ext2_inode.size();

        if offset >= file_size {
            return Ok(0);
        }

        let block_size = {
            let guard = EXT2_FS.lock();
            guard.as_ref().map(|fs| fs.block_size).unwrap_or(1024)
        };

        let avail = (file_size - offset) as usize;
        let to_read = buf.len().min(avail);
        let mut bytes_read = 0;

        while bytes_read < to_read {
            let file_offset = offset as usize + bytes_read;
            let block_idx = file_offset / block_size;
            let block_off = file_offset % block_size;

            if block_idx >= 12 {
                // Only direct blocks supported for now.
                break;
            }

            let block_num = ext2_inode.get_block(block_idx);
            if block_num == 0 {
                break;
            }

            let mut block_buf = vec![0u8; block_size];
            let lba = block_num as u64 * (block_size / 512) as u64;
            if !read_blocks(self.device_id, lba, block_size / 512, &mut block_buf) {
                return Err(FsError::IoError);
            }

            let chunk = (block_size - block_off).min(to_read - bytes_read);
            buf[bytes_read..bytes_read + chunk]
                .copy_from_slice(&block_buf[block_off..block_off + chunk]);
            bytes_read += chunk;
        }

        Ok(bytes_read)
    }

    fn write(&self, _inode: InodeId, _offset: u64, _buf: &[u8]) -> Result<usize, FsError> {
        Err(FsError::PermissionDenied)
    }

    fn stat(&self, inode: InodeId) -> Result<InodeStat, FsError> {
        let ino = inode.0 as u32;
        let ext2_inode = read_inode(self.device_id, ino).ok_or(FsError::IoError)?;

        let mode_val = {
            let p = core::ptr::addr_of!(ext2_inode.mode);
            // Safety: ext2_inode is a valid packed struct; p points to its mode field.
            unsafe { core::ptr::read_unaligned(p) }
        };
        let uid_val = {
            let p = core::ptr::addr_of!(ext2_inode.uid);
            // Safety: ext2_inode is a valid packed struct; p points to its uid field.
            unsafe { core::ptr::read_unaligned(p) }
        };
        let gid_val = {
            let p = core::ptr::addr_of!(ext2_inode.gid);
            // Safety: ext2_inode is a valid packed struct; p points to its gid field.
            unsafe { core::ptr::read_unaligned(p) }
        };
        let nlink_val = {
            let p = core::ptr::addr_of!(ext2_inode.links_count);
            // Safety: ext2_inode is a valid packed struct; p points to its links_count field.
            unsafe { core::ptr::read_unaligned(p) }
        };
        let atime_val = {
            let p = core::ptr::addr_of!(ext2_inode.atime);
            // Safety: ext2_inode is a valid packed struct; p points to its atime field.
            unsafe { core::ptr::read_unaligned(p) }
        };
        let mtime_val = {
            let p = core::ptr::addr_of!(ext2_inode.mtime);
            // Safety: ext2_inode is a valid packed struct; p points to its mtime field.
            unsafe { core::ptr::read_unaligned(p) }
        };
        let ctime_val = {
            let p = core::ptr::addr_of!(ext2_inode.ctime);
            // Safety: ext2_inode is a valid packed struct; p points to its ctime field.
            unsafe { core::ptr::read_unaligned(p) }
        };

        Ok(InodeStat {
            mode: mode_val as u32,
            uid: uid_val as u32,
            gid: gid_val as u32,
            nlink: nlink_val as u32,
            atime: atime_val as u64,
            mtime: mtime_val as u64,
            ctime: ctime_val as u64,
            size: ext2_inode.size(),
            file_type: mode_to_file_type(mode_val),
        })
    }

    fn readdir(&self, inode: InodeId) -> Result<Vec<DirEntry>, FsError> {
        let ino = inode.0 as u32;
        let entries = list_dir(self.device_id, ino);
        if entries.is_empty() {
            let ext2_inode = read_inode(self.device_id, ino).ok_or(FsError::IoError)?;
            if !ext2_inode.is_directory() {
                return Err(FsError::NotADirectory);
            }
            return Ok(Vec::new());
        }
        Ok(entries
            .into_iter()
            .map(|(child_ino, name, ft)| DirEntry {
                inode: InodeId(child_ino as u64),
                name,
                file_type: dir_ftype_to_file_type(ft),
            })
            .collect())
    }

    fn mkdir(&self, _parent: InodeId, _name: &str, _mode: u32) -> Result<InodeId, FsError> {
        Err(FsError::PermissionDenied)
    }

    fn unlink(&self, _parent: InodeId, _name: &str) -> Result<(), FsError> {
        Err(FsError::PermissionDenied)
    }

    fn rename(
        &self,
        _old_parent: InodeId,
        _old_name: &str,
        _new_parent: InodeId,
        _new_name: &str,
    ) -> Result<(), FsError> {
        Err(FsError::PermissionDenied)
    }

    fn sync(&self) -> Result<(), FsError> {
        Ok(())
    }
}
