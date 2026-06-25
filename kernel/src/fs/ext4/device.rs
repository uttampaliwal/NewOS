//! Ext4 block device adapter.
//!
//! Reads ext4 on-disk structures (superblock, group descriptors, inodes)
//! through the `BlockDevice` trait.

extern crate alloc;

use super::disk::{BlockGroupDescriptor, DirEntry2, EXT4_ROOT_INO, Ext4Inode, Ext4Superblock};
use crate::block::{BlockDevice, SECTOR_SIZE};
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Adapter that reads ext4 structures from a block device.
pub struct Ext4Device {
    device: Arc<dyn BlockDevice>,
    sb: Ext4Superblock,
    /// Block size in bytes.
    block_size: u32,
    /// Group descriptor table start sector.
    gdts_sector: u64,
    /// Size of each group descriptor entry.
    desc_size: u16,
}

impl Ext4Device {
    /// Try to mount an ext4 filesystem on the given block device.
    ///
    /// Reads and validates the superblock, then locates the group descriptor table.
    pub fn open(device: Arc<dyn BlockDevice>) -> Result<Self, Ext4DeviceError> {
        // The superblock is at byte offset 1024 from the start of the partition.
        let sb_byte_offset: u64 = 1024;
        let sb_sector = sb_byte_offset / SECTOR_SIZE as u64;

        let mut sb_buf = alloc::vec![0u8; 1024];
        device
            .read_sectors(sb_sector, &mut sb_buf)
            .map_err(|_| Ext4DeviceError::ReadError)?;

        // Safety: sb_buf contains a valid superblock read from disk at the correct offset.
        let sb = unsafe { core::ptr::read(sb_buf.as_ptr() as *const Ext4Superblock) };

        sb.validate().map_err(Ext4DeviceError::Superblock)?;

        let block_size = sb.block_size();
        let desc_size = sb.desc_size();

        // Group descriptor table starts at the block after the superblock.
        // For 1K blocks: block 2 (byte 2048)
        // For 4K blocks: block 1 (byte 4096)
        let gdt_block = if block_size == 1024 { 2 } else { 1 };
        let gdts_sector = (gdt_block as u64 * block_size as u64) / SECTOR_SIZE as u64;

        Ok(Self {
            device,
            sb,
            block_size,
            gdts_sector,
            desc_size,
        })
    }

    /// Read a block from the device.
    pub fn read_block(&self, block: u64, buf: &mut [u8]) -> Result<(), Ext4DeviceError> {
        let sector = (block * self.block_size as u64) / SECTOR_SIZE as u64;
        self.device
            .read_sectors(sector, buf)
            .map_err(|_| Ext4DeviceError::ReadError)?;
        Ok(())
    }

    /// Write a block to the device.
    pub fn write_block(&self, block: u64, buf: &[u8]) -> Result<(), Ext4DeviceError> {
        let sector = (block * self.block_size as u64) / SECTOR_SIZE as u64;
        self.device
            .write_sectors(sector, buf)
            .map_err(|_| Ext4DeviceError::WriteError)?;
        Ok(())
    }

    /// Get the superblock reference.
    pub fn superblock(&self) -> &Ext4Superblock {
        &self.sb
    }

    /// Get the block size.
    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    /// Read a group descriptor by index.
    pub fn read_group_descriptor(
        &self,
        group: u32,
    ) -> Result<BlockGroupDescriptor, Ext4DeviceError> {
        let offset = group as u64 * self.desc_size as u64;
        let sector = self.gdts_sector + offset / SECTOR_SIZE as u64;
        let byte_in_sector = (offset % SECTOR_SIZE as u64) as usize;

        let mut buf = alloc::vec![0u8; SECTOR_SIZE];
        self.device
            .read_sectors(sector, &mut buf)
            .map_err(|_| Ext4DeviceError::ReadError)?;

        // Safety: buf contains valid group descriptor data read from disk; byte_in_sector is within bounds.
        let bgd = unsafe {
            core::ptr::read(buf.as_ptr().add(byte_in_sector) as *const BlockGroupDescriptor)
        };
        Ok(bgd)
    }

    /// Read an inode by number.
    pub fn read_inode(&self, inode_num: u32) -> Result<Ext4Inode, Ext4DeviceError> {
        if inode_num == 0 {
            return Err(Ext4DeviceError::InvalidInode);
        }

        let inodes_per_group = self.sb.inodes_per_group();
        let group = (inode_num - 1) / inodes_per_group;
        let index = (inode_num - 1) % inodes_per_group;

        let bgd = self.read_group_descriptor(group)?;
        let inode_table_block = bgd.inode_table_block() as u64;
        let inode_size = self.sb.inode_size() as u64;
        let inode_offset = index as u64 * inode_size;

        let byte_in_block = inode_offset % self.block_size as u64;
        let block = inode_table_block + inode_offset / self.block_size as u64;

        let mut block_buf = alloc::vec![0u8; self.block_size as usize];
        self.read_block(block, &mut block_buf)?;

        // Safety: block_buf contains valid inode table data read from disk; byte_in_block is within bounds.
        let inode = unsafe {
            core::ptr::read(block_buf.as_ptr().add(byte_in_block as usize) as *const Ext4Inode)
        };

        Ok(inode)
    }

    /// Read the root inode (inode 2).
    pub fn read_root_inode(&self) -> Result<Ext4Inode, Ext4DeviceError> {
        self.read_inode(EXT4_ROOT_INO)
    }

    /// Read directory entries from a directory inode.
    pub fn read_dir_entries(&self, inode: &Ext4Inode) -> Result<Vec<DirEntry2>, Ext4DeviceError> {
        let size = inode.size() as usize;
        let mut entries = Vec::new();
        let mut offset = 0usize;

        while offset < size {
            let block_index = offset / self.block_size as usize;
            let byte_in_block = offset % self.block_size as usize;

            let phys_block = self.get_inode_block(inode, block_index as u32)?;
            let mut block_buf = alloc::vec![0u8; self.block_size as usize];
            self.read_block(phys_block, &mut block_buf)?;

            let mut pos = byte_in_block;
            while pos < self.block_size as usize {
                let remaining = &block_buf[pos..];
                if remaining.len() < 8 {
                    break;
                }

                let rec_inode =
                    u32::from_le_bytes([remaining[0], remaining[1], remaining[2], remaining[3]]);
                let rec_len = u16::from_le_bytes([remaining[4], remaining[5]]);
                let name_len = remaining[6];
                let file_type = remaining[7];

                if rec_len == 0 || rec_len as usize > self.block_size as usize - pos {
                    break;
                }

                if rec_inode != 0 {
                    let name_start = 8;
                    let name_end = name_start + name_len as usize;
                    let name = if name_end <= remaining.len() {
                        remaining[name_start..name_end].to_vec()
                    } else {
                        Vec::new()
                    };

                    entries.push(DirEntry2 {
                        inode: rec_inode,
                        rec_len,
                        name_len,
                        file_type,
                        name,
                    });
                }

                offset += rec_len as usize;
                pos += rec_len as usize;
            }
        }

        Ok(entries)
    }

    /// Get the physical block number for a logical block within an inode.
    /// Uses direct blocks, single indirect, double indirect, and triple indirect.
    pub fn get_inode_block(
        &self,
        inode: &Ext4Inode,
        logical_block: u32,
    ) -> Result<u64, Ext4DeviceError> {
        let blocks_per_block = self.block_size / 4;

        if logical_block < 12 {
            let phys = inode.i_block[logical_block as usize];
            Ok(phys as u64)
        } else if logical_block < 12 + blocks_per_block {
            let indirect_block = inode.i_block[12] as u64;
            if indirect_block == 0 {
                return Err(Ext4DeviceError::SparseBlock);
            }
            let mut buf = alloc::vec![0u8; self.block_size as usize];
            self.read_block(indirect_block, &mut buf)?;
            let idx = (logical_block - 12) as usize;
            let phys = u32::from_le_bytes([
                buf[idx * 4],
                buf[idx * 4 + 1],
                buf[idx * 4 + 2],
                buf[idx * 4 + 3],
            ]);
            Ok(phys as u64)
        } else if logical_block < 12 + blocks_per_block + blocks_per_block * blocks_per_block {
            let double_indirect = inode.i_block[13] as u64;
            if double_indirect == 0 {
                return Err(Ext4DeviceError::SparseBlock);
            }
            let idx1 = (logical_block - 12 - blocks_per_block) / blocks_per_block;
            let idx2 = (logical_block - 12 - blocks_per_block) % blocks_per_block;

            let mut buf1 = alloc::vec![0u8; self.block_size as usize];
            self.read_block(double_indirect, &mut buf1)?;
            let indirect_block = u32::from_le_bytes([
                buf1[idx1 as usize * 4],
                buf1[idx1 as usize * 4 + 1],
                buf1[idx1 as usize * 4 + 2],
                buf1[idx1 as usize * 4 + 3],
            ]) as u64;

            if indirect_block == 0 {
                return Err(Ext4DeviceError::SparseBlock);
            }
            let mut buf2 = alloc::vec![0u8; self.block_size as usize];
            self.read_block(indirect_block, &mut buf2)?;
            let phys = u32::from_le_bytes([
                buf2[idx2 as usize * 4],
                buf2[idx2 as usize * 4 + 1],
                buf2[idx2 as usize * 4 + 2],
                buf2[idx2 as usize * 4 + 3],
            ]);
            Ok(phys as u64)
        } else {
            Err(Ext4DeviceError::TooLarge)
        }
    }
}

/// Errors from ext4 device operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ext4DeviceError {
    ReadError,
    WriteError,
    InvalidInode,
    SparseBlock,
    TooLarge,
    Superblock(super::disk::SuperblockError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockDeviceId, BlockError};
    use alloc::sync::Arc;

    /// Minimal mock for testing ext4 device parsing.
    struct MockBlockDevice {
        data: spin::Mutex<Vec<u8>>,
    }

    impl MockBlockDevice {
        fn new(sectors: u64) -> Self {
            Self {
                data: spin::Mutex::new(alloc::vec![0u8; sectors as usize * SECTOR_SIZE]),
            }
        }

        /// Write a superblock at the correct location (byte 1024).
        fn write_superblock(&self, sb: &Ext4Superblock) {
            let mut data = self.data.lock();
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    sb as *const Ext4Superblock as *const u8,
                    core::mem::size_of::<Ext4Superblock>(),
                )
            };
            data[1024..1024 + bytes.len()].copy_from_slice(bytes);
        }
    }

    impl BlockDevice for MockBlockDevice {
        fn read_sectors(&self, sector: u64, buf: &mut [u8]) -> Result<usize, BlockError> {
            let data = self.data.lock();
            let offset = sector as usize * SECTOR_SIZE;
            if offset + buf.len() > data.len() {
                return Err(BlockError::OutOfRange);
            }
            buf.copy_from_slice(&data[offset..offset + buf.len()]);
            Ok(buf.len() / SECTOR_SIZE)
        }

        fn write_sectors(&self, sector: u64, buf: &[u8]) -> Result<usize, BlockError> {
            let mut data = self.data.lock();
            let offset = sector as usize * SECTOR_SIZE;
            if offset + buf.len() > data.len() {
                return Err(BlockError::OutOfRange);
            }
            data[offset..offset + buf.len()].copy_from_slice(buf);
            Ok(buf.len() / SECTOR_SIZE)
        }

        fn flush(&self) -> Result<(), BlockError> {
            Ok(())
        }
        fn total_sectors(&self) -> u64 {
            self.data.lock().len() as u64 / SECTOR_SIZE as u64
        }
        fn device_name(&self) -> &str {
            "mock-ext4"
        }
        fn device_id(&self) -> BlockDeviceId {
            BlockDeviceId(42)
        }
    }

    #[test]
    fn test_open_reads_superblock() {
        let dev = Arc::new(MockBlockDevice::new(8192));

        let mut sb: Ext4Superblock = unsafe { core::mem::zeroed() };
        sb.s_magic = super::super::disk::EXT4_SUPER_MAGIC;
        sb.s_inode_size = 256;
        sb.s_blocks_per_group = 8192;
        sb.s_inodes_per_group = 2048;
        sb.s_log_block_size = 2;
        sb.s_first_data_block = 0;
        sb.s_blocks_count_lo = 8192;
        sb.s_first_ino = 11;

        dev.write_superblock(&sb);

        let ext4 = Ext4Device::open(dev).unwrap();
        let magic = ext4.superblock().s_magic;
        assert_eq!(magic, super::super::disk::EXT4_SUPER_MAGIC);
        assert_eq!(ext4.block_size(), 4096);
    }

    #[test]
    fn test_open_rejects_bad_magic() {
        let dev = Arc::new(MockBlockDevice::new(8192));
        let result = Ext4Device::open(dev);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_inode_root() {
        let dev = Arc::new(MockBlockDevice::new(8192));

        let mut sb: Ext4Superblock = unsafe { core::mem::zeroed() };
        sb.s_magic = super::super::disk::EXT4_SUPER_MAGIC;
        sb.s_inode_size = 256;
        sb.s_blocks_per_group = 8192;
        sb.s_inodes_per_group = 2048;
        sb.s_log_block_size = 2;
        sb.s_first_data_block = 0;
        sb.s_blocks_count_lo = 8192;
        sb.s_first_ino = 11;

        dev.write_superblock(&sb);

        let ext4 = Ext4Device::open(dev).unwrap();
        let root = ext4.read_root_inode();
        assert!(root.is_ok());
    }
}
