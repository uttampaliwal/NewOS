//! ext2 write operations.
//!
//! Implements file creation, modification, and deletion for ext2.

extern crate alloc;

use alloc::vec::Vec;
use super::allocator::{AllocError, GroupAllocator};
use super::disk::{DirEntry2, Ext4Inode};

/// Write operations for ext2 files.
pub struct Ext2Writer {
    /// Per-group allocators.
    groups: Vec<GroupAllocator>,
    /// Blocks per group.
    blocks_per_group: u32,
    /// Inodes per group.
    inodes_per_group: u32,
    /// Block size in bytes.
    _block_size: u32,
}

impl Ext2Writer {
    pub fn new(
        block_groups: u32,
        blocks_per_group: u32,
        inodes_per_group: u32,
        block_size: u32,
    ) -> Self {
        let groups = (0..block_groups)
            .map(|_| GroupAllocator::new(blocks_per_group, inodes_per_group))
            .collect();

        Self {
            groups,
            blocks_per_group,
            inodes_per_group,
            _block_size: block_size,
        }
    }

    /// Allocate a free block across all groups.
    pub fn alloc_block(&mut self) -> Result<u32, AllocError> {
        for (i, group) in self.groups.iter_mut().enumerate() {
            if group.block_bitmap.free_count() > 0 {
                let group_start = i as u32 * self.blocks_per_group;
                return group.alloc_block(group_start);
            }
        }
        Err(AllocError::NoFreeBlocks)
    }

    /// Free a block.
    pub fn free_block(&mut self, block: u32) -> Result<(), AllocError> {
        let group_idx = (block / self.blocks_per_group) as usize;
        let group_start = group_idx as u32 * self.blocks_per_group;
        self.groups[group_idx].free_block(block, group_start)
    }

    /// Allocate a free inode across all groups.
    pub fn alloc_inode(&mut self) -> Result<u32, AllocError> {
        for (i, group) in self.groups.iter_mut().enumerate() {
            if group.inode_bitmap.free_count() > 0 {
                return group.alloc_inode(i as u32, self.inodes_per_group);
            }
        }
        Err(AllocError::NoFreeInodes)
    }

    /// Free an inode.
    pub fn free_inode(&mut self, inode: u32) -> Result<(), AllocError> {
        let group_idx = ((inode - 1) / self.inodes_per_group) as usize;
        self.groups[group_idx].free_inode(inode, group_idx as u32, self.inodes_per_group)
    }

    /// Create a new directory entry in a directory inode.
    /// Returns the new directory entry.
    pub fn create_dir_entry(
        &self,
        _parent_inode: u32,
        child_inode: u32,
        name: &str,
        file_type: u8,
    ) -> DirEntry2 {
        let name_bytes = name.as_bytes();
        let name_len = core::cmp::min(name_bytes.len(), 255) as u8;
        let rec_len = DirEntry2::total_rec_len(name_len);

        DirEntry2 {
            inode: child_inode,
            rec_len,
            name_len,
            file_type,
            name: name_bytes[..name_len as usize].to_vec(),
        }
    }

    /// Serialize a directory entry to bytes.
    pub fn serialize_dir_entry(entry: &DirEntry2) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&entry.inode.to_le_bytes());
        buf.extend_from_slice(&entry.rec_len.to_le_bytes());
        buf.push(entry.name_len);
        buf.push(entry.file_type);
        buf.extend_from_slice(&entry.name);
        // Pad to rec_len
        let pad = entry.rec_len as usize - buf.len();
        buf.extend(core::iter::repeat_n(0, pad));
        buf
    }

    /// Initialize a new inode.
    pub fn init_inode(&self, inode_num: u32, mode: u16, uid: u16, gid: u16) -> Ext4Inode {
        let mut inode: Ext4Inode = unsafe { core::mem::zeroed() };
        inode.i_mode = mode;
        inode.i_uid = uid;
        inode.i_gid = gid;
        inode.i_links_count = 1;
        inode.i_blocks_lo = 0;
        inode.i_generation = inode_num;
        inode
    }

    /// Calculate total free blocks across all groups.
    pub fn total_free_blocks(&self) -> u32 {
        self.groups.iter().map(|g| g.block_bitmap.free_count()).sum()
    }

    /// Calculate total free inodes across all groups.
    pub fn total_free_inodes(&self) -> u32 {
        self.groups.iter().map(|g| g.inode_bitmap.free_count()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_block_round_robin() {
        let mut writer = Ext2Writer::new(2, 8, 8, 4096);
        let b1 = writer.alloc_block().unwrap();
        let b2 = writer.alloc_block().unwrap();
        assert_eq!(b1, 0); // Group 0, block 0
        assert_eq!(b2, 1); // Group 0, block 1
    }

    #[test]
    fn test_alloc_block_crosses_groups() {
        let mut writer = Ext2Writer::new(2, 2, 8, 4096);
        let b1 = writer.alloc_block().unwrap();
        let b2 = writer.alloc_block().unwrap();
        let b3 = writer.alloc_block().unwrap(); // Should be in group 1
        assert_eq!(b1, 0);
        assert_eq!(b2, 1);
        assert_eq!(b3, 2); // Group 1, block 0
    }

    #[test]
    fn test_free_block() {
        let mut writer = Ext2Writer::new(1, 8, 8, 4096);
        let b = writer.alloc_block().unwrap();
        writer.free_block(b).unwrap();
        assert_eq!(writer.total_free_blocks(), 8);
    }

    #[test]
    fn test_alloc_inode() {
        let mut writer = Ext2Writer::new(2, 8, 8, 4096);
        let i1 = writer.alloc_inode().unwrap();
        let i2 = writer.alloc_inode().unwrap();
        assert_eq!(i1, 1);
        assert_eq!(i2, 2);
    }

    #[test]
    fn test_alloc_inode_crosses_groups() {
        let mut writer = Ext2Writer::new(2, 8, 4, 4096);
        for _ in 0..4 {
            writer.alloc_inode().unwrap();
        }
        let i5 = writer.alloc_inode().unwrap();
        assert_eq!(i5, 5); // Group 1, first inode
    }

    #[test]
    fn test_dir_entry_creation() {
        let writer = Ext2Writer::new(1, 8, 8, 4096);
        let entry = writer.create_dir_entry(1, 2, "test.txt", DirEntry2::EXT4_FT_REG_FILE);
        assert_eq!(entry.inode, 2);
        assert_eq!(entry.name, b"test.txt");
        assert_eq!(entry.file_type, DirEntry2::EXT4_FT_REG_FILE);
    }

    #[test]
    fn test_dir_entry_serialize() {
        let writer = Ext2Writer::new(1, 8, 8, 4096);
        let entry = writer.create_dir_entry(1, 2, "ab", DirEntry2::EXT4_FT_REG_FILE);
        let bytes = Ext2Writer::serialize_dir_entry(&entry);
        assert!(bytes.len() <= entry.rec_len as usize);
    }

    #[test]
    fn test_init_inode() {
        let writer = Ext2Writer::new(1, 8, 8, 4096);
        let inode = writer.init_inode(5, 0o100644, 1000, 1000);
        let mode = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(inode.i_mode)) };
        let uid = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(inode.i_uid)) };
        let links = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(inode.i_links_count)) };
        assert_eq!(mode, 0o100644);
        assert_eq!(uid, 1000);
        assert_eq!(links, 1);
    }

    #[test]
    fn test_total_free() {
        let mut writer = Ext2Writer::new(3, 16, 32, 4096);
        assert_eq!(writer.total_free_blocks(), 48); // 3 * 16
        assert_eq!(writer.total_free_inodes(), 96); // 3 * 32

        writer.alloc_block().unwrap();
        writer.alloc_inode().unwrap();

        assert_eq!(writer.total_free_blocks(), 47);
        assert_eq!(writer.total_free_inodes(), 95);
    }

    #[test]
    fn test_exhaustion() {
        let mut writer = Ext2Writer::new(1, 4, 4, 4096);
        for _ in 0..4 {
            writer.alloc_block().unwrap();
        }
        assert_eq!(writer.alloc_block(), Err(AllocError::NoFreeBlocks));
    }
}
