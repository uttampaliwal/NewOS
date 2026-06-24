//! ext4 in-memory state management.
//!
//! Manages the in-memory representation of ext4 filesystem state,
//! including inode cache, directory entries, file data, and dirty tracking.
//! This layer sits between the VFS and the on-disk Ext4Device.

extern crate alloc;

use super::disk::Ext4Inode;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// In-memory inode representation.
#[derive(Debug, Clone)]
pub struct MemInode {
    /// On-disk inode metadata.
    pub inode: Ext4Inode,
    /// File data blocks (logical block index -> data).
    /// For directories, this stores serialized directory entries.
    pub data: Vec<u8>,
    /// Extended attributes.
    pub xattrs: BTreeMap<String, Vec<u8>>,
    /// Whether this inode has been modified since last sync.
    pub dirty: bool,
}

impl MemInode {
    /// Create a new regular file inode.
    pub fn new_file(mode: u16, uid: u16, gid: u16) -> Self {
        let mut inode: Ext4Inode = unsafe { core::mem::zeroed() };
        inode.i_mode = mode;
        inode.i_uid = uid;
        inode.i_gid = gid;
        inode.i_links_count = 1;
        Self {
            inode,
            data: Vec::new(),
            xattrs: BTreeMap::new(),
            dirty: true,
        }
    }

    /// Create a new directory inode with `.` and `..` entries.
    pub fn new_dir(mode: u16, uid: u16, gid: u16, self_ino: u32, parent_ino: u32) -> Self {
        let mut inode: Ext4Inode = unsafe { core::mem::zeroed() };
        inode.i_mode = mode | 0o040000;
        inode.i_uid = uid;
        inode.i_gid = gid;
        inode.i_links_count = 2; // `.` + parent's `..`
        inode.i_blocks_lo = 1; // One block for directory entries

        let mut data = Vec::new();
        // Append `.` entry
        append_dir_entry_raw(&mut data, self_ino, ".", 0o040000);
        // Append `..` entry
        append_dir_entry_raw(&mut data, parent_ino, "..", 0o040000);

        Self {
            inode,
            data,
            xattrs: BTreeMap::new(),
            dirty: true,
        }
    }

    /// File size in bytes.
    pub fn size(&self) -> u64 {
        self.inode.size()
    }

    /// Set file size.
    pub fn set_size(&mut self, size: u64) {
        self.inode.i_size_lo = (size & 0xFFFFFFFF) as u32;
        self.inode.i_size_high = (size >> 32) as u32;
        self.dirty = true;
    }

    /// Is this a directory?
    pub fn is_dir(&self) -> bool {
        self.inode.is_dir()
    }

    /// Is this a regular file?
    pub fn is_file(&self) -> bool {
        self.inode.is_file()
    }
}

/// In-memory directory entry (for readdir results).
#[derive(Debug, Clone)]
pub struct MemDirEntry {
    pub inode_num: u64,
    pub name: String,
    pub file_type: u8,
}

/// Core ext4 filesystem state.
///
/// Manages all in-memory state for an ext4 filesystem. When a block device
/// is provided, supports write-back to disk via `sync()`.
pub struct Ext4State {
    /// Inode number -> in-memory inode.
    inodes: BTreeMap<u64, MemInode>,
    /// Next inode number to allocate.
    next_inode: u64,
    /// Root inode number (usually 2).
    root_inode: u64,
    /// Global counter for allocated data blocks (for i_blocks accounting).
    #[allow(dead_code)]
    allocated_blocks: u32,
}

impl Ext4State {
    /// Create a new empty ext4 state (in-memory only, no block device).
    pub fn new() -> Self {
        let root_inode = 2u64;
        let mut inodes = BTreeMap::new();

        // Create root directory inode
        let root = MemInode::new_dir(0o0755, 0, 0, root_inode as u32, root_inode as u32);
        inodes.insert(root_inode, root);

        Self {
            inodes,
            next_inode: 3, // 1 = bad, 2 = root, next available = 3
            root_inode,
            allocated_blocks: 0,
        }
    }

    /// Return the root inode number.
    pub fn root_inode(&self) -> u64 {
        self.root_inode
    }

    /// Look up an inode by number.
    pub fn get_inode(&self, ino: u64) -> Option<&MemInode> {
        self.inodes.get(&ino)
    }

    /// Look up an inode by number (mutable).
    pub fn get_inode_mut(&mut self, ino: u64) -> Option<&mut MemInode> {
        self.inodes.get_mut(&ino)
    }

    /// Allocate a new inode number.
    pub fn alloc_inode_num(&mut self) -> u64 {
        let ino = self.next_inode;
        self.next_inode += 1;
        ino
    }

    /// Create a new regular file inode.
    pub fn create_file(&mut self, mode: u16, uid: u16, gid: u16) -> u64 {
        let ino = self.alloc_inode_num();
        let inode = MemInode::new_file(mode, uid, gid);
        self.inodes.insert(ino, inode);
        ino
    }

    /// Create a new directory inode.
    pub fn create_dir(&mut self, mode: u16, uid: u16, gid: u16, parent_ino: u64) -> u64 {
        let ino = self.alloc_inode_num();
        let inode = MemInode::new_dir(mode, uid, gid, ino as u32, parent_ino as u32);
        self.inodes.insert(ino, inode);

        // Increment parent link count (for `..`)
        if let Some(parent) = self.inodes.get_mut(&parent_ino) {
            parent.inode.i_links_count = parent.inode.i_links_count.saturating_add(1);
            parent.dirty = true;
        }

        ino
    }

    /// Look up a child by name in a directory inode.
    pub fn lookup_child(&self, parent_ino: u64, name: &str) -> Option<u64> {
        let parent = self.inodes.get(&parent_ino)?;
        if !parent.is_dir() {
            return None;
        }

        let mut offset = 0usize;
        while offset < parent.data.len() {
            if offset + 8 > parent.data.len() {
                break;
            }
            let rec_inode = u32::from_le_bytes([
                parent.data[offset],
                parent.data[offset + 1],
                parent.data[offset + 2],
                parent.data[offset + 3],
            ]);
            let rec_len = u16::from_le_bytes([parent.data[offset + 4], parent.data[offset + 5]]);
            let name_len = parent.data[offset + 6];

            if rec_len == 0 {
                break;
            }

            let name_start = offset + 8;
            let name_end = name_start + name_len as usize;
            if name_end <= parent.data.len() && rec_inode != 0 {
                let entry_name =
                    core::str::from_utf8(&parent.data[name_start..name_end]).unwrap_or("");
                if entry_name == name {
                    return Some(rec_inode as u64);
                }
            }

            offset += rec_len as usize;
        }
        None
    }

    /// Add a directory entry to a directory inode.
    pub fn add_dir_entry(
        &mut self,
        parent_ino: u64,
        child_ino: u64,
        name: &str,
        file_type: u8,
    ) -> Result<(), &'static str> {
        let parent = self.inodes.get_mut(&parent_ino).ok_or("parent not found")?;
        if !parent.is_dir() {
            return Err("not a directory");
        }

        let mut entry_bytes = Vec::new();
        entry_bytes.extend_from_slice(&(child_ino as u32).to_le_bytes());
        let name_bytes = name.as_bytes();
        let name_len = core::cmp::min(name_bytes.len(), 255) as u8;
        let rec_len = ((8 + name_len as u16 + 3) & !3) as u16;
        entry_bytes.extend_from_slice(&rec_len.to_le_bytes());
        entry_bytes.push(name_len);
        entry_bytes.push(file_type);
        entry_bytes.extend_from_slice(&name_bytes[..name_len as usize]);
        let pad = rec_len as usize - entry_bytes.len();
        entry_bytes.extend(core::iter::repeat_n(0u8, pad));

        let parent = self.inodes.get_mut(&parent_ino).unwrap();
        parent.data.extend_from_slice(&entry_bytes);
        parent.dirty = true;

        // Update size to reflect directory data
        let new_size = parent.data.len() as u64;
        parent.inode.i_size_lo = (new_size & 0xFFFFFFFF) as u32;
        parent.inode.i_size_high = (new_size >> 32) as u32;

        Ok(())
    }

    /// Remove a directory entry by name.
    pub fn remove_dir_entry(&mut self, parent_ino: u64, name: &str) -> Result<u64, &'static str> {
        let parent = self.inodes.get_mut(&parent_ino).ok_or("parent not found")?;
        if !parent.is_dir() {
            return Err("not a directory");
        }

        let mut offset = 0usize;
        let mut found_inode = 0u64;
        while offset < parent.data.len() {
            if offset + 8 > parent.data.len() {
                break;
            }
            let rec_inode = u32::from_le_bytes([
                parent.data[offset],
                parent.data[offset + 1],
                parent.data[offset + 2],
                parent.data[offset + 3],
            ]);
            let rec_len = u16::from_le_bytes([parent.data[offset + 4], parent.data[offset + 5]]);
            let entry_name_len = parent.data[offset + 6];

            if rec_len == 0 {
                break;
            }

            let name_start = offset + 8;
            let name_end = name_start + entry_name_len as usize;
            if name_end <= parent.data.len() && rec_inode != 0 {
                let entry_name =
                    core::str::from_utf8(&parent.data[name_start..name_end]).unwrap_or("");
                if entry_name == name {
                    found_inode = rec_inode as u64;
                    // Zero out the inode number (mark as deleted), preserve rec_len for traversal
                    parent.data[offset..offset + 4].fill(0);
                    break;
                }
            }

            offset += rec_len as usize;
        }

        if found_inode == 0 {
            return Err("entry not found");
        }

        let parent = self.inodes.get_mut(&parent_ino).unwrap();
        parent.dirty = true;
        Ok(found_inode)
    }

    /// List directory entries.
    pub fn readdir(&self, ino: u64) -> Vec<MemDirEntry> {
        let mut entries = Vec::new();
        let inode = match self.inodes.get(&ino) {
            Some(i) => i,
            None => return entries,
        };
        if !inode.is_dir() {
            return entries;
        }

        let mut offset = 0usize;
        while offset < inode.data.len() {
            if offset + 8 > inode.data.len() {
                break;
            }
            let rec_inode = u32::from_le_bytes([
                inode.data[offset],
                inode.data[offset + 1],
                inode.data[offset + 2],
                inode.data[offset + 3],
            ]);
            let rec_len = u16::from_le_bytes([inode.data[offset + 4], inode.data[offset + 5]]);
            let name_len = inode.data[offset + 6];
            let file_type = inode.data[offset + 7];

            if rec_len == 0 {
                break;
            }

            let name_start = offset + 8;
            let name_end = name_start + name_len as usize;
            if name_end <= inode.data.len() && rec_inode != 0 {
                let name = String::from_utf8_lossy(&inode.data[name_start..name_end]).to_string();
                entries.push(MemDirEntry {
                    inode_num: rec_inode as u64,
                    name,
                    file_type,
                });
            }

            offset += rec_len as usize;
        }

        entries
    }

    /// Read data from a file inode.
    pub fn read_data(&self, ino: u64, offset: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
        let inode = self.inodes.get(&ino).ok_or("inode not found")?;
        if inode.is_dir() {
            return Err("is a directory");
        }

        let file_size = inode.size() as usize;
        let start = offset as usize;
        if start >= file_size {
            return Ok(0);
        }

        let end = core::cmp::min(start + buf.len(), file_size);
        let len = end - start;
        buf[..len].copy_from_slice(&inode.data[start..end]);
        Ok(len)
    }

    /// Write data to a file inode.
    pub fn write_data(&mut self, ino: u64, offset: u64, buf: &[u8]) -> Result<usize, &'static str> {
        let inode = self.inodes.get_mut(&ino).ok_or("inode not found")?;
        if inode.is_dir() {
            return Err("is a directory");
        }

        let start = offset as usize;
        let end = start + buf.len();

        // Extend data vector if needed
        if end > inode.data.len() {
            inode.data.resize(end, 0);
        }

        inode.data[start..end].copy_from_slice(buf);

        // Update file size
        let new_size = end as u64;
        let old_size = inode.size();
        if new_size > old_size {
            inode.inode.i_size_lo = (new_size & 0xFFFFFFFF) as u32;
            inode.inode.i_size_high = (new_size >> 32) as u32;
        }

        inode.dirty = true;
        Ok(buf.len())
    }

    /// Truncate a file to the given size.
    pub fn truncate(&mut self, ino: u64, size: u64) -> Result<(), &'static str> {
        let inode = self.inodes.get_mut(&ino).ok_or("inode not found")?;
        if inode.is_dir() {
            return Err("is a directory");
        }

        let new_len = size as usize;
        if new_len < inode.data.len() {
            inode.data.truncate(new_len);
        } else {
            inode.data.resize(new_len, 0);
        }

        inode.inode.i_size_lo = (size & 0xFFFFFFFF) as u32;
        inode.inode.i_size_high = (size >> 32) as u32;
        inode.dirty = true;
        Ok(())
    }

    /// Check if an inode exists.
    pub fn inode_exists(&self, ino: u64) -> bool {
        self.inodes.contains_key(&ino)
    }

    /// Remove an inode entirely.
    pub fn remove_inode(&mut self, ino: u64) -> Option<MemInode> {
        self.inodes.remove(&ino)
    }

    /// Get the count of inodes.
    pub fn inode_count(&self) -> usize {
        self.inodes.len()
    }

    /// Get total size of all file data (for statfs).
    pub fn total_data_size(&self) -> u64 {
        self.inodes.values().map(|i| i.data.len() as u64).sum()
    }

    /// Get mutable iterator over all inodes.
    pub fn inodes_mut(&mut self) -> impl Iterator<Item = (&u64, &mut MemInode)> {
        self.inodes.iter_mut()
    }
}

impl Default for Ext4State {
    fn default() -> Self {
        Self::new()
    }
}

/// Append a raw directory entry to a byte buffer.
fn append_dir_entry_raw(data: &mut Vec<u8>, inode_num: u32, name: &str, mode: u16) {
    let name_bytes = name.as_bytes();
    let name_len = core::cmp::min(name_bytes.len(), 255) as u8;
    let rec_len = ((8 + name_len as u16 + 3) & !3) as u16;
    let entry_start = data.len();

    data.extend_from_slice(&inode_num.to_le_bytes());
    data.extend_from_slice(&rec_len.to_le_bytes());
    data.push(name_len);

    let file_type = if mode & 0o170000 == 0o040000 {
        super::disk::DirEntry2::EXT4_FT_DIR
    } else {
        super::disk::DirEntry2::EXT4_FT_REG_FILE
    };
    data.push(file_type);
    data.extend_from_slice(&name_bytes[..name_len as usize]);
    let pad = rec_len as usize - (data.len() - entry_start);
    data.extend(core::iter::repeat_n(0u8, pad));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_state_has_root() {
        let state = Ext4State::new();
        assert!(state.inode_exists(2));
        assert!(state.get_inode(2).unwrap().is_dir());
    }

    #[test]
    fn test_create_file() {
        let mut state = Ext4State::new();
        let ino = state.create_file(0o100644, 1000, 1000);
        assert!(state.inode_exists(ino));
        assert!(state.get_inode(ino).unwrap().is_file());
    }

    #[test]
    fn test_create_dir() {
        let mut state = Ext4State::new();
        let ino = state.create_dir(0o0755, 0, 0, 2);
        assert!(state.inode_exists(ino));
        assert!(state.get_inode(ino).unwrap().is_dir());
        // Parent should have `..` link count incremented
        let links = state.get_inode(2).unwrap().inode.i_links_count;
        assert_eq!(links, 3);
    }

    #[test]
    fn test_lookup_child() {
        let mut state = Ext4State::new();
        let file_ino = state.create_file(0o100644, 0, 0);
        state.add_dir_entry(2, file_ino, "test.txt", 1).unwrap();
        assert_eq!(state.lookup_child(2, "test.txt"), Some(file_ino));
        assert_eq!(state.lookup_child(2, "nonexistent"), None);
    }

    #[test]
    fn test_write_read_data() {
        let mut state = Ext4State::new();
        let ino = state.create_file(0o100644, 0, 0);
        state.write_data(ino, 0, b"hello world").unwrap();
        let mut buf = [0u8; 11];
        let n = state.read_data(ino, 0, &mut buf).unwrap();
        assert_eq!(n, 11);
        assert_eq!(&buf, b"hello world");
    }

    #[test]
    fn test_readdir() {
        let mut state = Ext4State::new();
        let file_ino = state.create_file(0o100644, 0, 0);
        state.add_dir_entry(2, file_ino, "a.txt", 1).unwrap();
        let entries = state.readdir(2);
        assert!(entries.iter().any(|e| e.name == "a.txt"));
    }

    #[test]
    fn test_remove_dir_entry() {
        let mut state = Ext4State::new();
        let file_ino = state.create_file(0o100644, 0, 0);
        state.add_dir_entry(2, file_ino, "del.txt", 1).unwrap();
        let removed = state.remove_dir_entry(2, "del.txt").unwrap();
        assert_eq!(removed, file_ino);
        assert_eq!(state.lookup_child(2, "del.txt"), None);
    }

    #[test]
    fn test_truncate() {
        let mut state = Ext4State::new();
        let ino = state.create_file(0o100644, 0, 0);
        state.write_data(ino, 0, b"hello world").unwrap();
        state.truncate(ino, 5).unwrap();
        assert_eq!(state.get_inode(ino).unwrap().size(), 5);
    }
}
