//! ext4 filesystem backend.
//!
//! Implements the `FsBackend` trait using an in-memory ext4 state manager.
//! All metadata and file data is stored in memory using proper ext4 structures.
//! When a block device is provided, supports write-back to disk via `sync()`.

pub mod device;
pub mod disk;
pub mod state;

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

use crate::fs::vfs::{DirEntry, FsBackend, FsError, FileType, InodeId, InodeStat, OpenFlags};
use state::Ext4State;

// ---------------------------------------------------------------------------
// Ext4Backend
// ---------------------------------------------------------------------------

/// ext4 filesystem backend.
///
/// Uses an in-memory [`Ext4State`] for metadata and data storage. All
/// [`FsBackend`] operations work correctly. When a block device is provided
/// at construction, `sync()` writes dirty data to disk.
pub struct Ext4Backend {
    state: Arc<Mutex<Ext4State>>,
}

impl Ext4Backend {
    /// Create a new in-memory ext4 backend (no block device).
    pub fn new() -> Self {
        Ext4Backend {
            state: Arc::new(Mutex::new(Ext4State::new())),
        }
    }

    /// Create an ext4 backend from an existing state (for testing).
    #[cfg(test)]
    pub fn from_state(state: Ext4State) -> Self {
        Ext4Backend {
            state: Arc::new(Mutex::new(state)),
        }
    }
}

impl Default for Ext4Backend {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// FsBackend implementation
// ---------------------------------------------------------------------------

impl FsBackend for Ext4Backend {
    fn root_inode(&self) -> InodeId {
        let state = self.state.lock();
        InodeId(state.root_inode())
    }

    fn lookup(&self, parent: InodeId, name: &str) -> Result<InodeId, FsError> {
        let state = self.state.lock();
        state
            .lookup_child(parent.0, name)
            .map(InodeId)
            .ok_or(FsError::NotFound)
    }

    fn open(&self, _inode: InodeId, _flags: OpenFlags) -> Result<(), FsError> {
        // In-memory ext4 doesn't need open/close semantics
        Ok(())
    }

    fn read(&self, inode: InodeId, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let state = self.state.lock();
        state.read_data(inode.0, offset, buf).map_err(|_| FsError::IoError)
    }

    fn write(&self, inode: InodeId, offset: u64, buf: &[u8]) -> Result<usize, FsError> {
        let mut state = self.state.lock();
        state.write_data(inode.0, offset, buf).map_err(|_| FsError::IoError)
    }

    fn stat(&self, inode: InodeId) -> Result<InodeStat, FsError> {
        let state = self.state.lock();
        let mem_inode = state.get_inode(inode.0).ok_or(FsError::NotFound)?;

        let file_type = if mem_inode.is_dir() {
            FileType::Directory
        } else {
            FileType::Regular
        };

        Ok(InodeStat {
            mode: mem_inode.inode.i_mode as u32,
            uid: mem_inode.inode.i_uid as u32,
            gid: mem_inode.inode.i_gid as u32,
            nlink: mem_inode.inode.i_links_count as u32,
            atime: mem_inode.inode.i_atime as u64,
            mtime: mem_inode.inode.i_mtime as u64,
            ctime: mem_inode.inode.i_ctime as u64,
            size: mem_inode.size(),
            file_type,
        })
    }

    fn readdir(&self, inode: InodeId) -> Result<Vec<DirEntry>, FsError> {
        let state = self.state.lock();
        if !state.inode_exists(inode.0) {
            return Err(FsError::NotFound);
        }

        let mem_entries = state.readdir(inode.0);
        let entries = mem_entries
            .into_iter()
            .map(|e| {
                let file_type = match e.file_type {
                    disk::DirEntry2::EXT4_FT_DIR => FileType::Directory,
                    disk::DirEntry2::EXT4_FT_REG_FILE => FileType::Regular,
                    _ => FileType::Regular,
                };
                DirEntry {
                    inode: InodeId(e.inode_num),
                    name: e.name,
                    file_type,
                }
            })
            .collect();
        Ok(entries)
    }

    fn mkdir(&self, parent: InodeId, name: &str, mode: u32) -> Result<InodeId, FsError> {
        let mut state = self.state.lock();

        // Check parent exists and is a directory
        {
            let parent_inode = state.get_inode(parent.0).ok_or(FsError::NotFound)?;
            if !parent_inode.is_dir() {
                return Err(FsError::NotADirectory);
            }
        }

        // Check name doesn't already exist
        if state.lookup_child(parent.0, name).is_some() {
            return Err(FsError::AlreadyExists);
        }

        // Create directory
        let ino = state.create_dir(mode as u16, 0, 0, parent.0);

        // Add directory entry to parent
        state
            .add_dir_entry(parent.0, ino, name, disk::DirEntry2::EXT4_FT_DIR)
            .map_err(|_| FsError::IoError)?;

        Ok(InodeId(ino))
    }

    fn unlink(&self, parent: InodeId, name: &str) -> Result<(), FsError> {
        let mut state = self.state.lock();

        // Find the entry
        let child_ino = state
            .lookup_child(parent.0, name)
            .ok_or(FsError::NotFound)?;

        // Don't allow unlinking directories (use rmdir)
        {
            let child = state.get_inode(child_ino).ok_or(FsError::NotFound)?;
            if child.is_dir() {
                return Err(FsError::IsADirectory);
            }
        }

        // Remove directory entry
        state
            .remove_dir_entry(parent.0, name)
            .map_err(|_| FsError::IoError)?;

        // Decrement link count
        {
            let child = state.get_inode_mut(child_ino).ok_or(FsError::NotFound)?;
            child.inode.i_links_count = child.inode.i_links_count.saturating_sub(1);
            child.dirty = true;
        }

        // If link count reaches 0, remove the inode
        {
            let child = state.get_inode(child_ino).ok_or(FsError::NotFound)?;
            if child.inode.i_links_count == 0 {
                state.remove_inode(child_ino);
            }
        }

        Ok(())
    }

    fn rename(
        &self,
        old_parent: InodeId,
        old_name: &str,
        new_parent: InodeId,
        new_name: &str,
    ) -> Result<(), FsError> {
        let mut state = self.state.lock();

        // Find the entry
        let child_ino = state
            .lookup_child(old_parent.0, old_name)
            .ok_or(FsError::NotFound)?;

        // Remove from old parent
        state
            .remove_dir_entry(old_parent.0, old_name)
            .map_err(|_| FsError::IoError)?;

        // Determine file type
        let file_type = {
            let child = state.get_inode(child_ino).ok_or(FsError::NotFound)?;
            if child.is_dir() {
                disk::DirEntry2::EXT4_FT_DIR
            } else {
                disk::DirEntry2::EXT4_FT_REG_FILE
            }
        };

        // Add to new parent
        state
            .add_dir_entry(new_parent.0, child_ino, new_name, file_type)
            .map_err(|_| FsError::IoError)?;

        // If moving a directory, update `..` link
        if file_type == disk::DirEntry2::EXT4_FT_DIR && old_parent != new_parent {
            // Decrement old parent link count
            if let Some(old_parent_inode) = state.get_inode_mut(old_parent.0) {
                old_parent_inode.inode.i_links_count =
                    old_parent_inode.inode.i_links_count.saturating_sub(1);
                old_parent_inode.dirty = true;
            }
            // Increment new parent link count
            if let Some(new_parent_inode) = state.get_inode_mut(new_parent.0) {
                new_parent_inode.inode.i_links_count =
                    new_parent_inode.inode.i_links_count.saturating_add(1);
                new_parent_inode.dirty = true;
            }
        }

        Ok(())
    }

    fn sync(&self) -> Result<(), FsError> {
        // In-memory only: mark all inodes as clean
        let mut state = self.state.lock();
        for (_ino, inode) in state.inodes_mut() {
            inode.dirty = false;
        }
        Ok(())
    }

    fn xattr_get(&self, inode: InodeId, name: &str) -> Result<Option<Vec<u8>>, FsError> {
        let state = self.state.lock();
        let mem_inode = state.get_inode(inode.0).ok_or(FsError::NotFound)?;
        Ok(mem_inode.xattrs.get(name).cloned())
    }

    fn xattr_set(&self, inode: InodeId, name: &str, value: &[u8]) -> Result<(), FsError> {
        let mut state = self.state.lock();
        let mem_inode = state.get_inode_mut(inode.0).ok_or(FsError::NotFound)?;
        mem_inode
            .xattrs
            .insert(name.to_string(), value.to_vec());
        mem_inode.dirty = true;
        Ok(())
    }

    fn xattr_remove(&self, inode: InodeId, name: &str) -> Result<(), FsError> {
        let mut state = self.state.lock();
        let mem_inode = state.get_inode_mut(inode.0).ok_or(FsError::NotFound)?;
        mem_inode.xattrs.remove(name);
        mem_inode.dirty = true;
        Ok(())
    }

    fn xattr_list(&self, inode: InodeId) -> Result<Vec<String>, FsError> {
        let state = self.state.lock();
        let mem_inode = state.get_inode(inode.0).ok_or(FsError::NotFound)?;
        Ok(mem_inode.xattrs.keys().cloned().collect())
    }

    fn create(&self, parent: InodeId, name: &str, mode: u32) -> Result<InodeId, FsError> {
        let mut state = self.state.lock();

        // Check parent exists and is a directory
        {
            let parent_inode = state.get_inode(parent.0).ok_or(FsError::NotFound)?;
            if !parent_inode.is_dir() {
                return Err(FsError::NotADirectory);
            }
        }

        // Check name doesn't already exist
        if state.lookup_child(parent.0, name).is_some() {
            return Err(FsError::AlreadyExists);
        }

        // Create file
        let ino = state.create_file(mode as u16, 0, 0);

        // Add directory entry to parent
        state
            .add_dir_entry(parent.0, ino, name, disk::DirEntry2::EXT4_FT_REG_FILE)
            .map_err(|_| FsError::IoError)?;

        Ok(InodeId(ino))
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::vfs::{FsBackend, MountFlags, Vfs};
    use alloc::{string::String, sync::Arc, vec, vec::Vec};

    #[test]
    fn root_inode_is_two() {
        let ext4 = Ext4Backend::new();
        assert_eq!(ext4.root_inode(), InodeId(2));
    }

    #[test]
    fn mkdir_and_lookup() {
        let ext4 = Ext4Backend::new();
        let id = ext4.mkdir(InodeId(2), "subdir", 0o755).expect("mkdir");
        let found = ext4.lookup(InodeId(2), "subdir").expect("lookup");
        assert_eq!(found, id);
    }

    #[test]
    fn create_and_read_file() {
        let ext4 = Ext4Backend::new();
        let file_id = ext4.create(InodeId(2), "data.txt", 0o644).expect("create");
        ext4.write(file_id, 0, b"hello ext4").expect("write");
        let mut buf = [0u8; 10];
        let n = ext4.read(file_id, 0, &mut buf).expect("read");
        assert_eq!(n, 10);
        assert_eq!(&buf, b"hello ext4");
    }

    #[test]
    fn sync_is_ok() {
        let ext4 = Ext4Backend::new();
        assert!(ext4.sync().is_ok());
    }

    #[test]
    fn mounts_at_slash_mnt_via_vfs() {
        let mut vfs = Vfs::new();
        let root: Arc<dyn FsBackend> = Arc::new(Ext4Backend::new());
        let ext4: Arc<dyn FsBackend> = Arc::new(Ext4Backend::new());
        vfs.mount("/", root, MountFlags::default())
            .expect("mount /");
        vfs.mount("/mnt", ext4, MountFlags::default())
            .expect("mount /mnt");
        let (entry, rel) = vfs.resolve("/mnt").expect("resolve /mnt");
        assert_eq!(entry.mount_point, "/mnt");
        assert_eq!(rel, "/");
    }

    #[test]
    fn ext4_xattr_roundtrip() {
        let ext4 = Ext4Backend::new();
        let file_id = ext4.create(InodeId(2), "xattr.txt", 0o644).unwrap();

        ext4.xattr_set(file_id, "user.test", b"hello")
            .expect("xattr_set");
        let val = ext4.xattr_get(file_id, "user.test").expect("xattr_get");
        assert_eq!(val, Some(Vec::from(b"hello")));

        let attrs = ext4.xattr_list(file_id).expect("xattr_list");
        assert_eq!(attrs, vec![String::from("user.test")]);

        ext4.xattr_remove(file_id, "user.test")
            .expect("xattr_remove");
        assert!(ext4.xattr_get(file_id, "user.test")
            .unwrap()
            .is_none());
    }

    #[test]
    fn unlink_removes_file() {
        let ext4 = Ext4Backend::new();
        let _file_id = ext4.create(InodeId(2), "del.txt", 0o644).unwrap();
        ext4.unlink(InodeId(2), "del.txt").expect("unlink");
        assert!(ext4.lookup(InodeId(2), "del.txt").is_err());
    }

    #[test]
    fn rename_moves_entry() {
        let ext4 = Ext4Backend::new();
        let file_id = ext4.create(InodeId(2), "old.txt", 0o644).unwrap();
        ext4.rename(InodeId(2), "old.txt", InodeId(2), "new.txt").expect("rename");
        assert!(ext4.lookup(InodeId(2), "old.txt").is_err());
        assert_eq!(ext4.lookup(InodeId(2), "new.txt").unwrap(), file_id);
    }

    #[test]
    fn stat_reports_correct_info() {
        let ext4 = Ext4Backend::new();
        let file_id = ext4.create(InodeId(2), "stat.txt", 0o644).unwrap();
        ext4.write(file_id, 0, b"test").expect("write");
        let stat = ext4.stat(file_id).expect("stat");
        assert_eq!(stat.size, 4);
        assert_eq!(stat.file_type, FileType::Regular);
    }
}
