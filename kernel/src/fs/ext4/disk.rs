//! On-disk ext4 data structures.
//!
//! Based on the ext4 disk layout specification. All structures use
//! little-endian byte order as specified by the ext4 format.

/// ext4 superblock magic number.
pub const EXT4_SUPER_MAGIC: u16 = 0xEF53;

/// Extent tree depth 0 means this is a leaf node.
pub const EXTENT_EXTENT_DEPTH_LEAF: u16 = 0;

/// Inode constants.
pub const EXT4_ROOT_INO: u32 = 2;
pub const EXT4_GOOD_OLD_FIRST_INO: u32 = 11;

/// Ext4 superblock (on-disk layout, 1024 bytes starting at byte 1024).
///
/// Reference: <https://www.kernel.org/doc/html/latest/filesystems/ext4/super.html>
#[derive(Debug, Clone)]
#[repr(C, packed)]
pub struct Ext4Superblock {
    pub s_inodes_count: u32,
    pub s_blocks_count_lo: u32,
    pub s_r_blocks_count_lo: u32,
    pub s_free_blocks_count_lo: u32,
    pub s_free_inodes_count: u32,
    pub s_first_data_block: u32,
    pub s_log_block_size: u32,
    pub s_log_cluster_size: u32,
    pub s_blocks_per_group: u32,
    pub s_clusters_per_group: u32,
    pub s_inodes_per_group: u32,
    pub s_mtime: u32,
    pub s_wtime: u32,
    pub s_mnt_count: u16,
    pub s_max_mnt_count: u16,
    pub s_magic: u16,
    pub s_state: u16,
    pub s_errors: u16,
    pub s_minor_rev_level: u16,
    pub s_lastcheck: u32,
    pub s_checkinterval: u32,
    pub s_creator_os: u32,
    pub s_rev_level: u32,
    pub s_def_resuid: u16,
    pub s_def_resgid: u16,
    // EXT4_DYNAMIC_REV fields
    pub s_first_ino: u32,
    pub s_inode_size: u16,
    pub s_block_group_nr: u16,
    pub s_feature_compat: u32,
    pub s_feature_incompat: u32,
    pub s_feature_ro_compat: u32,
    pub s_uuid: [u8; 16],
    pub s_volume_name: [u8; 16],
    pub s_last_mounted: [u8; 64],
    pub s_algorithm_usage_bitmap: u32,
    // Preallocation fields
    pub s_prealloc_blocks: u8,
    pub s_prealloc_dir_blocks: u8,
    pub s_reserved_gdt_blocks: u16,
    // Journal fields
    pub s_journal_inum: u32,
    pub s_journal_dev: u32,
    pub s_last_orphan: u32,
    // Directory index fields
    pub s_hash_seed: [u32; 4],
    pub s_def_hash_version: u8,
    pub s_jnl_backup_type: u8,
    pub s_desc_size: u16,
    pub s_default_mount_opts: u32,
    pub s_first_meta_bg: u32,
    // Reserved
    pub _reserved: [u8; 360],
    // Backup superblock locations
    pub s_snapshot_inum: u32,
    pub s_snapshot_id: u32,
    pub s_snapshot_r_blocks_count: u64,
    pub s_snapshot_list: u32,
    // EXT4_FEATURE fields
    pub s_feature_compat2: u32,
    pub s_last_write_time: u32,
    pub s_last_check_time: u32,
    pub s_first_error_ino: u32,
    pub s_first_error_block: u64,
    pub s_first_error_func: [u8; 32],
    pub s_first_error_line: u32,
    pub s_last_error_ino: u32,
    pub s_last_error_block: u64,
    pub s_last_error_func: [u8; 32],
    pub s_last_error_line: u32,
    pub s_mount_opts: [u8; 64],
    pub s_usr_quota_inum: u32,
    pub s_grp_quota_inum: u32,
    pub s_overhead_clusters: u32,
    pub s_backup_bgs: [u32; 2],
    pub s_encrypt_algos: [u8; 4],
    pub s_encrypt_pw_salt: [u8; 16],
    pub s_lpf_ino: u32,
    pub s_prj_quota_inum: u32,
    pub s_checksum_seed: u32,
}

impl Ext4Superblock {
    /// The block size in bytes, derived from s_log_block_size.
    pub fn block_size(&self) -> u32 {
        1024u32 << self.s_log_block_size
    }

    /// The inode size in bytes.
    pub fn inode_size(&self) -> u16 {
        self.s_inode_size
    }

    /// Number of inodes per group.
    pub fn inodes_per_group(&self) -> u32 {
        self.s_inodes_per_group
    }

    /// Number of blocks per group.
    pub fn blocks_per_group(&self) -> u32 {
        self.s_blocks_per_group
    }

    /// Total block count (low 32 bits).
    pub fn blocks_count_lo(&self) -> u32 {
        self.s_blocks_count_lo
    }

    /// Number of first data block (0 for 4K blocks, 1 for 1K blocks).
    pub fn first_data_block(&self) -> u32 {
        self.s_first_data_block
    }

    /// Number of block groups.
    pub fn block_group_count(&self) -> u32 {
        let blocks = self.s_blocks_count_lo as u64;
        let bpg = self.s_blocks_per_group as u64;
        blocks.div_ceil(bpg) as u32
    }

    /// Descriptor size (bytes per group descriptor).
    pub fn desc_size(&self) -> u16 {
        if self.s_feature_incompat & 0x80 != 0 {
            // 64BIT feature — desc_size is in s_desc_size
            self.s_desc_size
        } else {
            32 // Legacy 32-byte descriptors
        }
    }

    /// Validate the superblock magic and basic consistency.
    pub fn validate(&self) -> Result<(), SuperblockError> {
        if self.s_magic != EXT4_SUPER_MAGIC {
            return Err(SuperblockError::BadMagic);
        }
        if self.s_inode_size == 0 || self.s_inode_size > 1024 {
            return Err(SuperblockError::InvalidInodeSize);
        }
        if self.s_blocks_per_group == 0 {
            return Err(SuperblockError::InvalidBlocksPerGroup);
        }
        if self.s_inodes_per_group == 0 {
            return Err(SuperblockError::InvalidInodesPerGroup);
        }
        Ok(())
    }
}

/// Errors from superblock parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuperblockError {
    BadMagic,
    InvalidInodeSize,
    InvalidBlocksPerGroup,
    InvalidInodesPerGroup,
    ReadError,
}

/// Block group descriptor (32-byte legacy format).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct BlockGroupDescriptor {
    pub bg_block_bitmap_lo: u32,
    pub bg_inode_bitmap_lo: u32,
    pub bg_inode_table_lo: u32,
    pub bg_free_blocks_count_lo: u16,
    pub bg_free_inodes_count_lo: u16,
    pub bg_used_dirs_count_lo: u16,
    pub bg_flags: u16,
    pub bg_exclude_bitmap_lo: u32,
    pub bg_block_bitmap_csum_lo: u16,
    pub bg_inode_bitmap_csum_lo: u16,
    pub bg_itable_unused_lo: u16,
    pub bg_checksum: u16,
}

impl BlockGroupDescriptor {
    pub fn block_bitmap_block(&self) -> u32 {
        self.bg_block_bitmap_lo
    }

    pub fn inode_bitmap_block(&self) -> u32 {
        self.bg_inode_bitmap_lo
    }

    pub fn inode_table_block(&self) -> u32 {
        self.bg_inode_table_lo
    }

    pub fn free_blocks(&self) -> u16 {
        self.bg_free_blocks_count_lo
    }

    pub fn free_inodes(&self) -> u16 {
        self.bg_free_inodes_count_lo
    }
}

/// On-disk inode structure (128 bytes for ext2/3/4).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Ext4Inode {
    pub i_mode: u16,
    pub i_uid: u16,
    pub i_size_lo: u32,
    pub i_atime: u32,
    pub i_ctime: u32,
    pub i_mtime: u32,
    pub i_dtime: u32,
    pub i_gid: u16,
    pub i_links_count: u16,
    pub i_blocks_lo: u32,
    pub i_flags: u32,
    pub i_osd1: u32,
    pub i_block: [u32; 15], // 12 direct + 1 indirect + 1 double indirect + 1 triple
    pub i_generation: u32,
    pub i_file_acl_lo: u32,
    pub i_size_high: u32,
    pub i_obso_fsize: u32,
    pub i_osd2: [u8; 12],
    pub i_extra_isize: u16,
    pub i_checksum_hi: u16,
    pub i_ctime_extra: u32,
    pub i_mtime_extra: u32,
    pub i_atime_extra: u32,
    pub i_crtime: u32,
    pub i_crtime_extra: u32,
    pub i_version_hi: u32,
}

impl Ext4Inode {
    /// Full file size (combining lo and high parts).
    pub fn size(&self) -> u64 {
        ((self.i_size_high as u64) << 32) | (self.i_size_lo as u64)
    }

    /// Check if this inode is a directory.
    pub fn is_dir(&self) -> bool {
        self.i_mode & 0o170000 == 0o040000
    }

    /// Check if this inode is a regular file.
    pub fn is_file(&self) -> bool {
        self.i_mode & 0o170000 == 0o100000
    }

    /// Check if this inode is a symlink.
    pub fn is_symlink(&self) -> bool {
        self.i_mode & 0o170000 == 0o120000
    }

    /// Permission bits (rwxrwxrwx + sticky/setuid/setgid).
    pub fn mode(&self) -> u16 {
        self.i_mode
    }

    /// Number of hard links.
    pub fn links_count(&self) -> u16 {
        self.i_links_count
    }

    /// Owner user ID.
    pub fn uid(&self) -> u16 {
        self.i_uid
    }

    /// Owner group ID.
    pub fn gid(&self) -> u16 {
        self.i_gid
    }

    /// Access time.
    pub fn atime(&self) -> u32 {
        self.i_atime
    }

    /// Modification time.
    pub fn mtime(&self) -> u32 {
        self.i_mtime
    }

    /// Creation time.
    pub fn ctime(&self) -> u32 {
        self.i_ctime
    }
}

/// Directory entry on disk (ext4 uses variable-length entries).
#[derive(Debug, Clone)]
pub struct DirEntry2 {
    pub inode: u32,
    pub rec_len: u16,
    pub name_len: u8,
    pub file_type: u8,
    pub name: alloc::vec::Vec<u8>,
}

impl DirEntry2 {
    /// File type values stored in d_type.
    pub const EXT4_FT_UNKNOWN: u8 = 0;
    pub const EXT4_FT_REG_FILE: u8 = 1;
    pub const EXT4_FT_DIR: u8 = 2;
    pub const EXT4_FT_CHRDEV: u8 = 3;
    pub const EXT4_FT_BLKDEV: u8 = 4;
    pub const EXT4_FT_FIFO: u8 = 5;
    pub const EXT4_FT_SOCK: u8 = 6;
    pub const EXT4_FT_SYMLINK: u8 = 7;

    /// The minimum record length (8 bytes for entry header without name).
    pub const MIN_REC_LEN: u16 = 8;

    /// Total record length (aligned to 4-byte boundary).
    pub fn total_rec_len(name_len: u8) -> u16 {
        let base = 8 + name_len as u16;
        (base + 3) & !3 // Align to 4 bytes
    }
}

/// Extent tree header.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct ExtentHeader {
    pub eh_magic: u16, // Should be 0xF30A
    pub eh_entries: u16,
    pub eh_max: u16,
    pub eh_depth: u16, // 0 = leaf, >0 = index node
    pub eh_generation: u32,
}

impl ExtentHeader {
    pub const MAGIC: u16 = 0xF30A;

    pub fn is_valid(&self) -> bool {
        self.eh_magic == Self::MAGIC
    }

    pub fn is_leaf(&self) -> bool {
        self.eh_depth == 0
    }
}

/// Extent tree index entry (internal node).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct ExtentIdx {
    pub ei_block: u32,   // Logical block number covered
    pub ei_leaf_lo: u32, // Low 32 bits of physical block of child node
    pub ei_leaf_hi: u16, // High 16 bits of physical block
    pub ei_unused: u16,
}

/// Extent tree leaf entry (data extent).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Extent {
    pub ee_block: u32,    // First logical block
    pub ee_len: u16,      // Number of blocks
    pub ee_start_hi: u16, // High 16 bits of physical start block
    pub ee_start_lo: u32, // Low 32 bits of physical start block
}

impl Extent {
    /// Physical start block.
    pub fn physical_block(&self) -> u64 {
        ((self.ee_start_hi as u64) << 32) | (self.ee_start_lo as u64)
    }

    /// Logical start block.
    pub fn logical_block(&self) -> u32 {
        self.ee_block
    }

    /// Number of blocks in this extent.
    pub fn length(&self) -> u16 {
        self.ee_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_superblock_block_size() {
        // Safety: Ext4Superblock is a plain data struct; zeroed() produces a valid all-zero instance.
        let mut sb: Ext4Superblock = unsafe { core::mem::zeroed() };
        sb.s_magic = EXT4_SUPER_MAGIC;
        sb.s_log_block_size = 2; // 1024 << 2 = 4096
        assert_eq!(sb.block_size(), 4096);
    }

    #[test]
    fn test_superblock_block_size_1k() {
        // Safety: Ext4Superblock is a plain data struct; zeroed() produces a valid all-zero instance.
        let mut sb: Ext4Superblock = unsafe { core::mem::zeroed() };
        sb.s_magic = EXT4_SUPER_MAGIC;
        sb.s_log_block_size = 0; // 1024 << 0 = 1024
        assert_eq!(sb.block_size(), 1024);
    }

    #[test]
    fn test_superblock_validate_good() {
        // Safety: Ext4Superblock is a plain data struct; zeroed() produces a valid all-zero instance.
        let mut sb: Ext4Superblock = unsafe { core::mem::zeroed() };
        sb.s_magic = EXT4_SUPER_MAGIC;
        sb.s_inode_size = 256;
        sb.s_blocks_per_group = 8192;
        sb.s_inodes_per_group = 2048;
        assert_eq!(sb.validate(), Ok(()));
    }

    #[test]
    fn test_superblock_validate_bad_magic() {
        // Safety: Ext4Superblock is a plain data struct; zeroed() produces a valid all-zero instance.
        let mut sb: Ext4Superblock = unsafe { core::mem::zeroed() };
        sb.s_magic = 0xBAD0;
        sb.s_inode_size = 256;
        sb.s_blocks_per_group = 8192;
        sb.s_inodes_per_group = 2048;
        assert_eq!(sb.validate(), Err(SuperblockError::BadMagic));
    }

    #[test]
    fn test_inode_is_dir() {
        // Safety: Ext4Inode is a plain data struct; zeroed() produces a valid all-zero instance.
        let mut inode: Ext4Inode = unsafe { core::mem::zeroed() };
        inode.i_mode = 0o040755; // directory
        assert!(inode.is_dir());
        assert!(!inode.is_file());
    }

    #[test]
    fn test_inode_is_file() {
        // Safety: Ext4Inode is a plain data struct; zeroed() produces a valid all-zero instance.
        let mut inode: Ext4Inode = unsafe { core::mem::zeroed() };
        inode.i_mode = 0o100644; // regular file
        assert!(inode.is_file());
        assert!(!inode.is_dir());
    }

    #[test]
    fn test_extent_header() {
        let header = ExtentHeader {
            eh_magic: ExtentHeader::MAGIC,
            eh_entries: 1,
            eh_max: 4,
            eh_depth: 0,
            eh_generation: 42,
        };
        assert!(header.is_valid());
        assert!(header.is_leaf());
    }

    #[test]
    fn test_extent_physical_block() {
        let ext = Extent {
            ee_block: 0,
            ee_len: 8,
            ee_start_hi: 0,
            ee_start_lo: 100,
        };
        assert_eq!(ext.physical_block(), 100);
        assert_eq!(ext.logical_block(), 0);
        assert_eq!(ext.length(), 8);
    }

    #[test]
    fn test_dir_entry_min_rec_len() {
        assert_eq!(DirEntry2::MIN_REC_LEN, 8);
    }

    #[test]
    fn test_dir_entry_total_rec_len() {
        // 8 (header) + 4 (name) = 12, aligned to 4 = 12
        assert_eq!(DirEntry2::total_rec_len(4), 12);
        // 8 + 5 = 13, aligned to 4 = 16
        assert_eq!(DirEntry2::total_rec_len(5), 16);
    }

    #[test]
    fn test_block_group_descriptor() {
        let bgd = BlockGroupDescriptor {
            bg_block_bitmap_lo: 100,
            bg_inode_bitmap_lo: 101,
            bg_inode_table_lo: 102,
            bg_free_blocks_count_lo: 8000,
            bg_free_inodes_count_lo: 2000,
            bg_used_dirs_count_lo: 50,
            bg_flags: 0,
            bg_exclude_bitmap_lo: 0,
            bg_block_bitmap_csum_lo: 0,
            bg_inode_bitmap_csum_lo: 0,
            bg_itable_unused_lo: 0,
            bg_checksum: 0,
        };
        assert_eq!(bgd.block_bitmap_block(), 100);
        assert_eq!(bgd.inode_bitmap_block(), 101);
        assert_eq!(bgd.inode_table_block(), 102);
        assert_eq!(bgd.free_blocks(), 8000);
        assert_eq!(bgd.free_inodes(), 2000);
    }
}
