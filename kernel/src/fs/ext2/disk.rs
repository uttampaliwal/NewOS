//! Re-export of ext4 on-disk types used by the ext2 write path.
//!
//! This module provides access to the shared on-disk data structures
//! (inodes, directory entries) defined in `crate::fs::ext4::disk`.

pub use crate::fs::ext4::disk::{DirEntry2, Ext4Inode};
