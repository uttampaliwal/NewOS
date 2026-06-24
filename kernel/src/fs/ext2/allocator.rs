//! ext2/ext4 block and inode bitmap allocator.
//!
//! Manages free block and inode bitmaps for each block group.
//! Supports allocation, deallocation, and consistency checking.

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

/// Allocator error types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocError {
    /// No free blocks available.
    NoFreeBlocks,
    /// No free inodes available.
    NoFreeInodes,
    /// Block number out of range.
    OutOfRange,
    /// Bitmap inconsistency.
    BitmapCorruption,
}

/// Block bitmap allocator for a single block group.
#[derive(Debug, Clone)]
pub struct BlockBitmap {
    /// Bitmap data (1 bit per block).
    data: Vec<u8>,
    /// Total number of blocks in this group.
    total_blocks: u32,
    /// Number of free blocks.
    free_count: u32,
}

impl BlockBitmap {
    /// Create a new block bitmap with all blocks free.
    pub fn new(total_blocks: u32) -> Self {
        let bytes = (total_blocks as usize).div_ceil(8);
        Self {
            data: vec![0xFF; bytes], // All bits set = all blocks free
            total_blocks,
            free_count: total_blocks,
        }
    }

    /// Create from existing bitmap data.
    pub fn from_data(data: Vec<u8>, total_blocks: u32) -> Self {
        let free_count = data
            .iter()
            .flat_map(|byte| (0..8).map(move |bit| (byte >> bit) & 1))
            .take(total_blocks as usize)
            .filter(|&b| b == 1)
            .count() as u32;

        Self {
            data,
            total_blocks,
            free_count,
        }
    }

    /// Check if a block is free.
    pub fn is_free(&self, block: u32) -> bool {
        if block >= self.total_blocks {
            return false;
        }
        let byte = (block / 8) as usize;
        let bit = block % 8;
        (self.data[byte] >> bit) & 1 == 1
    }

    /// Allocate a free block. Returns the block number.
    pub fn allocate(&mut self) -> Result<u32, AllocError> {
        for block in 0..self.total_blocks {
            if self.is_free(block) {
                // Set bit to 0 (allocated)
                let byte = (block / 8) as usize;
                let bit = block % 8;
                self.data[byte] &= !(1 << bit);
                self.free_count -= 1;
                return Ok(block);
            }
        }
        Err(AllocError::NoFreeBlocks)
    }

    /// Free a previously allocated block.
    pub fn free(&mut self, block: u32) -> Result<(), AllocError> {
        if block >= self.total_blocks {
            return Err(AllocError::OutOfRange);
        }
        if self.is_free(block) {
            return Err(AllocError::BitmapCorruption); // Double free
        }
        let byte = (block / 8) as usize;
        let bit = block % 8;
        self.data[byte] |= 1 << bit;
        self.free_count += 1;
        Ok(())
    }

    /// Number of free blocks.
    pub fn free_count(&self) -> u32 {
        self.free_count
    }

    /// Total number of blocks.
    pub fn total_blocks(&self) -> u32 {
        self.total_blocks
    }

    /// Serialize bitmap to bytes.
    pub fn to_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Get mutable reference to raw bitmap data.
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
}

/// Inode bitmap allocator for a single block group.
#[derive(Debug, Clone)]
pub struct InodeBitmap {
    data: Vec<u8>,
    total_inodes: u32,
    free_count: u32,
}

impl InodeBitmap {
    pub fn new(total_inodes: u32) -> Self {
        let bytes = (total_inodes as usize).div_ceil(8);
        Self {
            data: vec![0xFF; bytes],
            total_inodes,
            free_count: total_inodes,
        }
    }

    pub fn from_data(data: Vec<u8>, total_inodes: u32) -> Self {
        let free_count = data
            .iter()
            .flat_map(|byte| (0..8).map(move |bit| (byte >> bit) & 1))
            .take(total_inodes as usize)
            .filter(|&b| b == 1)
            .count() as u32;

        Self {
            data,
            total_inodes,
            free_count,
        }
    }

    pub fn is_free(&self, inode: u32) -> bool {
        if inode == 0 || inode > self.total_inodes {
            return false;
        }
        let idx = (inode - 1) as usize; // Inodes are 1-based
        let byte = idx / 8;
        let bit = idx % 8;
        (self.data[byte] >> bit) & 1 == 1
    }

    pub fn allocate(&mut self) -> Result<u32, AllocError> {
        for inode in 1..=self.total_inodes {
            if self.is_free(inode) {
                let idx = (inode - 1) as usize;
                let byte = idx / 8;
                let bit = idx % 8;
                self.data[byte] &= !(1 << bit);
                self.free_count -= 1;
                return Ok(inode);
            }
        }
        Err(AllocError::NoFreeInodes)
    }

    pub fn free(&mut self, inode: u32) -> Result<(), AllocError> {
        if inode == 0 || inode > self.total_inodes {
            return Err(AllocError::OutOfRange);
        }
        let idx = (inode - 1) as usize;
        let byte = idx / 8;
        let bit = idx % 8;
        self.data[byte] |= 1 << bit;
        self.free_count += 1;
        Ok(())
    }

    pub fn free_count(&self) -> u32 {
        self.free_count
    }

    pub fn to_bytes(&self) -> &[u8] {
        &self.data
    }
}

/// Block group allocator state.
#[derive(Debug, Clone)]
pub struct GroupAllocator {
    /// Block bitmap for this group.
    pub block_bitmap: BlockBitmap,
    /// Inode bitmap for this group.
    pub inode_bitmap: InodeBitmap,
}

impl GroupAllocator {
    pub fn new(blocks_per_group: u32, inodes_per_group: u32) -> Self {
        Self {
            block_bitmap: BlockBitmap::new(blocks_per_group),
            inode_bitmap: InodeBitmap::new(inodes_per_group),
        }
    }

    /// Allocate a block from this group.
    pub fn alloc_block(&mut self, group_start: u32) -> Result<u32, AllocError> {
        let local = self.block_bitmap.allocate()?;
        Ok(group_start + local)
    }

    /// Free a block in this group.
    pub fn free_block(&mut self, block: u32, group_start: u32) -> Result<(), AllocError> {
        let local = block
            .checked_sub(group_start)
            .ok_or(AllocError::OutOfRange)?;
        self.block_bitmap.free(local)
    }

    /// Allocate an inode from this group.
    /// Returns the global inode number (group_offset * inodes_per_group + local).
    pub fn alloc_inode(
        &mut self,
        group_offset: u32,
        inodes_per_group: u32,
    ) -> Result<u32, AllocError> {
        let local = self.inode_bitmap.allocate()?;
        Ok(group_offset * inodes_per_group + local)
    }

    /// Free an inode.
    pub fn free_inode(
        &mut self,
        global_inode: u32,
        group_offset: u32,
        inodes_per_group: u32,
    ) -> Result<(), AllocError> {
        let local = global_inode - group_offset * inodes_per_group;
        self.inode_bitmap.free(local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_bitmap_new_all_free() {
        let bm = BlockBitmap::new(32);
        assert_eq!(bm.free_count(), 32);
        for i in 0..32 {
            assert!(bm.is_free(i));
        }
    }

    #[test]
    fn test_block_bitmap_allocate() {
        let mut bm = BlockBitmap::new(8);
        let block = bm.allocate().unwrap();
        assert_eq!(block, 0);
        assert!(!bm.is_free(0));
        assert!(bm.is_free(1));
        assert_eq!(bm.free_count(), 7);
    }

    #[test]
    fn test_block_bitmap_free() {
        let mut bm = BlockBitmap::new(8);
        let block = bm.allocate().unwrap();
        bm.free(block).unwrap();
        assert!(bm.is_free(0));
        assert_eq!(bm.free_count(), 8);
    }

    #[test]
    fn test_block_bitmap_double_free() {
        let mut bm = BlockBitmap::new(8);
        let block = bm.allocate().unwrap();
        bm.free(block).unwrap();
        assert_eq!(bm.free(block), Err(AllocError::BitmapCorruption));
    }

    #[test]
    fn test_block_bitmap_exhaustion() {
        let mut bm = BlockBitmap::new(4);
        for _ in 0..4 {
            bm.allocate().unwrap();
        }
        assert_eq!(bm.allocate(), Err(AllocError::NoFreeBlocks));
    }

    #[test]
    fn test_inode_bitmap_allocate_free() {
        let mut bm = InodeBitmap::new(16);
        let inode = bm.allocate().unwrap();
        assert_eq!(inode, 1);
        assert!(!bm.is_free(1));
        bm.free(inode).unwrap();
        assert!(bm.is_free(1));
    }

    #[test]
    fn test_inode_bitmap_starts_at_1() {
        let mut bm = InodeBitmap::new(8);
        // Inode 0 is invalid
        assert!(!bm.is_free(0));
        let inode = bm.allocate().unwrap();
        assert_eq!(inode, 1);
    }

    #[test]
    fn test_group_allocator_block() {
        let mut ga = GroupAllocator::new(32, 32);
        let block = ga.alloc_block(100).unwrap();
        assert_eq!(block, 100);
        ga.free_block(block, 100).unwrap();
    }

    #[test]
    fn test_group_allocator_inode() {
        let mut ga = GroupAllocator::new(32, 32);
        let inode = ga.alloc_inode(2, 32).unwrap();
        // Group 2, first inode = 2 * 32 + 1 = 65
        assert_eq!(inode, 65);
    }

    #[test]
    fn test_from_data() {
        let data = vec![0b10101010, 0b01010101]; // Alternating bits
        let bm = BlockBitmap::from_data(data, 16);
        assert_eq!(bm.free_count(), 8);
        assert!(!bm.is_free(0)); // bit 0 = 0
        assert!(bm.is_free(1)); // bit 1 = 1
        assert!(!bm.is_free(2)); // bit 2 = 0
        assert!(bm.is_free(3)); // bit 3 = 1
    }
}
