//! tmpfs — in-memory filesystem backend for Turnix OS.
//!
//! `TmpfsBackend` implements [`FsBackend`] by storing all filesystem state in
//! a `BTreeMap<InodeId, TmpfsInode>` protected by a `spin::Mutex`.
//!
//! Design notes
//! - Root inode is always `InodeId(1)` — a directory.
//! - New inodes start at `InodeId(2)` and increment monotonically.
//! - File mode defaults: regular files = 0o644, directories = 0o755.
//! - All timestamps are 0 (no clock available in no_std kernel yet).
//! - `sync()` is a no-op — the backing store is RAM.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::fs::vfs::{
    DirEntry, FileType, FsBackend, FsError, InodeId, InodeStat, OpenFlags, Timestamp,
};

// ---------------------------------------------------------------------------
// Internal node representation
// ---------------------------------------------------------------------------

/// A single inode inside tmpfs.
struct TmpfsInode {
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub nlink: u32,
    pub atime: Timestamp,
    pub mtime: Timestamp,
    pub ctime: Timestamp,
    pub file_type: FileType,
    /// For `FileType::Regular` — the file data bytes.
    pub data: Vec<u8>,
    /// For `FileType::Directory` — child name → child InodeId.
    pub children: BTreeMap<String, InodeId>,
}

impl TmpfsInode {
    fn new_dir(mode: u32) -> Self {
        TmpfsInode {
            mode,
            uid: 0,
            gid: 0,
            nlink: 2,
            atime: 0,
            mtime: 0,
            ctime: 0,
            file_type: FileType::Directory,
            data: Vec::new(),
            children: BTreeMap::new(),
        }
    }

    #[allow(dead_code)]
    fn new_file(mode: u32) -> Self {
        TmpfsInode {
            mode,
            uid: 0,
            gid: 0,
            nlink: 1,
            atime: 0,
            mtime: 0,
            ctime: 0,
            file_type: FileType::Regular,
            data: Vec::new(),
            children: BTreeMap::new(),
        }
    }

    fn to_stat(&self) -> InodeStat {
        InodeStat {
            mode: self.mode,
            uid: self.uid,
            gid: self.gid,
            nlink: self.nlink,
            atime: self.atime,
            mtime: self.mtime,
            ctime: self.ctime,
            size: self.data.len() as u64,
            file_type: self.file_type,
        }
    }
}

// ---------------------------------------------------------------------------
// TmpfsInner — the mutable core behind the Mutex
// ---------------------------------------------------------------------------

pub(crate) struct TmpfsInner {
    inodes: BTreeMap<InodeId, TmpfsInode>,
    next_inode: u64,
}

impl TmpfsInner {
    fn new() -> Self {
        let mut inodes = BTreeMap::new();
        // Root directory is always inode 1.
        inodes.insert(InodeId(1), TmpfsInode::new_dir(0o755));
        TmpfsInner {
            inodes,
            next_inode: 2,
        }
    }

    pub(crate) fn alloc_inode(&mut self) -> InodeId {
        let id = InodeId(self.next_inode);
        self.next_inode += 1;
        id
    }

    /// Insert a regular file directly — used by tests and the ext4 stub.
    #[allow(dead_code)]
    pub(crate) fn create_file(
        &mut self,
        parent: InodeId,
        name: &str,
        mode: u32,
    ) -> Option<InodeId> {
        let parent_node = self.inodes.get_mut(&parent)?;
        if parent_node.file_type != FileType::Directory {
            return None;
        }
        let id = InodeId(self.next_inode);
        self.next_inode += 1;
        parent_node.children.insert(String::from(name), id);
        self.inodes.insert(id, TmpfsInode::new_file(mode));
        Some(id)
    }
}

// ---------------------------------------------------------------------------
// Public backend struct
// ---------------------------------------------------------------------------

/// In-memory filesystem backend.
///
/// All mutations go through `Mutex<TmpfsInner>` so the struct is `Send + Sync`
/// and can be wrapped in `Arc<dyn FsBackend>`.
pub struct TmpfsBackend {
    pub(crate) inner: Mutex<TmpfsInner>,
}

impl TmpfsBackend {
    /// Create a new, empty tmpfs rooted at inode 1.
    pub fn new() -> Self {
        TmpfsBackend {
            inner: Mutex::new(TmpfsInner::new()),
        }
    }
}

impl Default for TmpfsBackend {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// FsBackend implementation
// ---------------------------------------------------------------------------

impl FsBackend for TmpfsBackend {
    fn root_inode(&self) -> InodeId {
        InodeId(1)
    }

    fn lookup(&self, parent: InodeId, name: &str) -> Result<InodeId, FsError> {
        let inner = self.inner.lock();
        let parent_node = inner.inodes.get(&parent).ok_or(FsError::NotFound)?;
        if parent_node.file_type != FileType::Directory {
            return Err(FsError::NotADirectory);
        }
        parent_node
            .children
            .get(name)
            .copied()
            .ok_or(FsError::NotFound)
    }

    fn open(&self, inode: InodeId, flags: OpenFlags) -> Result<(), FsError> {
        let inner = self.inner.lock();
        let node = inner.inodes.get(&inode).ok_or(FsError::NotFound)?;
        if node.file_type == FileType::Directory && flags.writable() {
            return Err(FsError::IsADirectory);
        }
        Ok(())
    }

    fn read(&self, inode: InodeId, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let inner = self.inner.lock();
        let node = inner.inodes.get(&inode).ok_or(FsError::NotFound)?;
        if node.file_type == FileType::Directory {
            return Err(FsError::IsADirectory);
        }
        let start = offset as usize;
        if start >= node.data.len() {
            return Ok(0);
        }
        let avail = node.data.len() - start;
        let len = buf.len().min(avail);
        buf[..len].copy_from_slice(&node.data[start..start + len]);
        Ok(len)
    }

    fn write(&self, inode: InodeId, offset: u64, buf: &[u8]) -> Result<usize, FsError> {
        let mut inner = self.inner.lock();
        let node = inner.inodes.get_mut(&inode).ok_or(FsError::NotFound)?;
        if node.file_type == FileType::Directory {
            return Err(FsError::IsADirectory);
        }
        let start = offset as usize;
        let end = start + buf.len();
        if end > node.data.len() {
            node.data.resize(end, 0);
        }
        node.data[start..end].copy_from_slice(buf);
        Ok(buf.len())
    }

    fn stat(&self, inode: InodeId) -> Result<InodeStat, FsError> {
        let inner = self.inner.lock();
        let node = inner.inodes.get(&inode).ok_or(FsError::NotFound)?;
        Ok(node.to_stat())
    }

    fn readdir(&self, inode: InodeId) -> Result<Vec<DirEntry>, FsError> {
        let inner = self.inner.lock();
        let node = inner.inodes.get(&inode).ok_or(FsError::NotFound)?;
        if node.file_type != FileType::Directory {
            return Err(FsError::NotADirectory);
        }
        let mut entries = Vec::new();
        for (name, &child_id) in &node.children {
            let ft = inner
                .inodes
                .get(&child_id)
                .map(|n| n.file_type)
                .unwrap_or(FileType::Regular);
            entries.push(DirEntry {
                inode: child_id,
                name: name.clone(),
                file_type: ft,
            });
        }
        Ok(entries)
    }

    fn mkdir(&self, parent: InodeId, name: &str, mode: u32) -> Result<InodeId, FsError> {
        let mut inner = self.inner.lock();
        {
            let parent_node = inner.inodes.get(&parent).ok_or(FsError::NotFound)?;
            if parent_node.file_type != FileType::Directory {
                return Err(FsError::NotADirectory);
            }
            if parent_node.children.contains_key(name) {
                return Err(FsError::AlreadyExists);
            }
        }
        let new_mode = if mode == 0 { 0o755 } else { mode };
        let new_id = inner.alloc_inode();
        inner.inodes.insert(new_id, TmpfsInode::new_dir(new_mode));
        if let Some(parent_node) = inner.inodes.get_mut(&parent) {
            parent_node.children.insert(String::from(name), new_id);
        }
        Ok(new_id)
    }

    fn unlink(&self, parent: InodeId, name: &str) -> Result<(), FsError> {
        let mut inner = self.inner.lock();
        let child_id = {
            let parent_node = inner.inodes.get(&parent).ok_or(FsError::NotFound)?;
            if parent_node.file_type != FileType::Directory {
                return Err(FsError::NotADirectory);
            }
            *parent_node.children.get(name).ok_or(FsError::NotFound)?
        };
        // Refuse to unlink a non-empty directory.
        {
            let child_node = inner.inodes.get(&child_id).ok_or(FsError::NotFound)?;
            if child_node.file_type == FileType::Directory && !child_node.children.is_empty() {
                return Err(FsError::IsADirectory);
            }
        }
        if let Some(parent_node) = inner.inodes.get_mut(&parent) {
            parent_node.children.remove(name);
        }
        inner.inodes.remove(&child_id);
        Ok(())
    }

    fn rename(
        &self,
        old_parent: InodeId,
        old_name: &str,
        new_parent: InodeId,
        new_name: &str,
    ) -> Result<(), FsError> {
        let mut inner = self.inner.lock();
        let child_id = {
            let p = inner.inodes.get(&old_parent).ok_or(FsError::NotFound)?;
            if p.file_type != FileType::Directory {
                return Err(FsError::NotADirectory);
            }
            *p.children.get(old_name).ok_or(FsError::NotFound)?
        };
        {
            let np = inner.inodes.get(&new_parent).ok_or(FsError::NotFound)?;
            if np.file_type != FileType::Directory {
                return Err(FsError::NotADirectory);
            }
        }
        if let Some(p) = inner.inodes.get_mut(&old_parent) {
            p.children.remove(old_name);
        }
        if let Some(np) = inner.inodes.get_mut(&new_parent) {
            np.children.insert(String::from(new_name), child_id);
        }
        Ok(())
    }

    fn sync(&self) -> Result<(), FsError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::vfs::{MountFlags, Vfs};
    use alloc::sync::Arc;

    fn make_tmpfs() -> TmpfsBackend {
        TmpfsBackend::new()
    }

    /// Helper: insert a regular file directly into a TmpfsBackend.
    fn insert_file(fs: &TmpfsBackend, parent: InodeId, name: &str) -> InodeId {
        let mut inner = fs.inner.lock();
        let id = inner.alloc_inode();
        inner.inodes.insert(id, TmpfsInode::new_file(0o644));
        inner
            .inodes
            .get_mut(&parent)
            .unwrap()
            .children
            .insert(String::from(name), id);
        id
    }

    // -----------------------------------------------------------------------
    // Basic operations
    // -----------------------------------------------------------------------

    #[test]
    fn root_inode_is_one() {
        let fs = make_tmpfs();
        assert_eq!(fs.root_inode(), InodeId(1));
    }

    #[test]
    fn root_is_a_directory() {
        let fs = make_tmpfs();
        let stat = fs.stat(InodeId(1)).expect("stat root");
        assert_eq!(stat.file_type, FileType::Directory);
    }

    #[test]
    fn lookup_missing_returns_not_found() {
        let fs = make_tmpfs();
        assert_eq!(fs.lookup(InodeId(1), "missing"), Err(FsError::NotFound));
    }

    #[test]
    fn mkdir_creates_directory() {
        let fs = make_tmpfs();
        let id = fs.mkdir(InodeId(1), "subdir", 0o755).expect("mkdir");
        let stat = fs.stat(id).expect("stat");
        assert_eq!(stat.file_type, FileType::Directory);
    }

    #[test]
    fn lookup_finds_created_dir() {
        let fs = make_tmpfs();
        let id = fs.mkdir(InodeId(1), "subdir", 0o755).expect("mkdir");
        let found = fs.lookup(InodeId(1), "subdir").expect("lookup");
        assert_eq!(found, id);
    }

    #[test]
    fn mkdir_duplicate_returns_already_exists() {
        let fs = make_tmpfs();
        fs.mkdir(InodeId(1), "subdir", 0o755).expect("first mkdir");
        assert_eq!(
            fs.mkdir(InodeId(1), "subdir", 0o755),
            Err(FsError::AlreadyExists)
        );
    }

    #[test]
    fn write_then_read_file() {
        let fs = make_tmpfs();
        let file_id = insert_file(&fs, InodeId(1), "test.txt");
        let data = b"hello tmpfs";
        fs.write(file_id, 0, data).expect("write");
        let mut buf = [0u8; 11];
        let n = fs.read(file_id, 0, &mut buf).expect("read");
        assert_eq!(n, 11);
        assert_eq!(&buf, data);
    }

    #[test]
    fn write_at_offset_extends_file() {
        let fs = make_tmpfs();
        let file_id = insert_file(&fs, InodeId(1), "f.bin");
        fs.write(file_id, 0, b"hello").expect("write1");
        fs.write(file_id, 5, b"world").expect("write2");
        let mut buf = [0u8; 10];
        let n = fs.read(file_id, 0, &mut buf).expect("read");
        assert_eq!(n, 10);
        assert_eq!(&buf, b"helloworld");
    }

    #[test]
    fn read_beyond_eof_returns_zero() {
        let fs = make_tmpfs();
        let file_id = insert_file(&fs, InodeId(1), "empty.txt");
        let mut buf = [0u8; 4];
        let n = fs.read(file_id, 100, &mut buf).expect("read");
        assert_eq!(n, 0);
    }

    #[test]
    fn stat_reports_correct_size() {
        let fs = make_tmpfs();
        let file_id = insert_file(&fs, InodeId(1), "sized.txt");
        fs.write(file_id, 0, b"1234567890").expect("write");
        let stat = fs.stat(file_id).expect("stat");
        assert_eq!(stat.size, 10);
        assert_eq!(stat.file_type, FileType::Regular);
    }

    #[test]
    fn unlink_removes_entry() {
        let fs = make_tmpfs();
        fs.mkdir(InodeId(1), "dir_to_remove", 0o755).expect("mkdir");
        fs.unlink(InodeId(1), "dir_to_remove").expect("unlink");
        assert_eq!(
            fs.lookup(InodeId(1), "dir_to_remove"),
            Err(FsError::NotFound)
        );
    }

    #[test]
    fn rename_moves_entry() {
        let fs = make_tmpfs();
        let id = fs.mkdir(InodeId(1), "old_name", 0o755).expect("mkdir");
        fs.rename(InodeId(1), "old_name", InodeId(1), "new_name")
            .expect("rename");
        assert_eq!(fs.lookup(InodeId(1), "old_name"), Err(FsError::NotFound));
        assert_eq!(fs.lookup(InodeId(1), "new_name"), Ok(id));
    }

    #[test]
    fn readdir_lists_children() {
        let fs = make_tmpfs();
        fs.mkdir(InodeId(1), "a", 0o755).expect("mkdir a");
        fs.mkdir(InodeId(1), "b", 0o755).expect("mkdir b");
        let entries = fs.readdir(InodeId(1)).expect("readdir");
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
    }

    #[test]
    fn sync_is_noop() {
        let fs = make_tmpfs();
        assert!(fs.sync().is_ok());
    }

    // -----------------------------------------------------------------------
    // VFS integration: umount with open FDs returns BusyMounted (req 21.3)
    // -----------------------------------------------------------------------

    /// **Validates: Requirements 21.3**
    ///
    /// Mounting a TmpfsBackend and opening a file, then calling `umount`
    /// must return `FsError::BusyMounted`.  After closing the fd the umount
    /// must succeed.
    #[test]
    fn umount_with_open_fd_returns_busy() {
        let mut vfs = Vfs::new();
        let backend = TmpfsBackend::new();
        // Insert a file directly so we can open it.
        let _fid = insert_file(&backend, InodeId(1), "busy.txt");

        let backend_arc: Arc<dyn FsBackend> = Arc::new(backend);
        vfs.mount("/tmp", backend_arc, MountFlags::default())
            .expect("mount");

        let fd = vfs
            .open_with_creds("/tmp/busy.txt", OpenFlags::RDONLY, 0, 0)
            .expect("open");

        // Should be busy because fd is still open.
        assert_eq!(vfs.umount("/tmp"), Err(FsError::BusyMounted));

        vfs.close_fd(fd);
        // After closing fd, umount must succeed.
        assert!(vfs.umount("/tmp").is_ok());
    }

    /// **Validates: Requirements 21.2, 21.5**
    ///
    /// Path resolution must delegate to the correct mounted backend.
    /// A file that exists only in the `/tmp` backend should be accessible
    /// under `/tmp/...` but not under `/...`.
    #[test]
    fn path_resolution_delegates_to_correct_backend() {
        let mut vfs = Vfs::new();

        let root_backend = TmpfsBackend::new();
        let tmp_backend = TmpfsBackend::new();

        // Insert a file only in tmp_backend.
        insert_file(&tmp_backend, InodeId(1), "only_in_tmp.txt");

        vfs.mount(
            "/",
            Arc::new(root_backend) as Arc<dyn FsBackend>,
            MountFlags::default(),
        )
        .expect("mount /");
        vfs.mount(
            "/tmp",
            Arc::new(tmp_backend) as Arc<dyn FsBackend>,
            MountFlags::default(),
        )
        .expect("mount /tmp");

        // File is accessible under /tmp.
        let fd = vfs
            .open_with_creds("/tmp/only_in_tmp.txt", OpenFlags::RDONLY, 0, 0)
            .expect("open via /tmp mount");
        vfs.close_fd(fd);

        // File is NOT accessible under root.
        let result = vfs.open_with_creds("/only_in_tmp.txt", OpenFlags::RDONLY, 0, 0);
        assert!(result.is_err(), "file should not be in root backend");
    }
}
