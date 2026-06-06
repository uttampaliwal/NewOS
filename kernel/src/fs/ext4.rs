//! ext4 filesystem backend for Turnix OS.
//!
//! # Current status
//!
//! A fully block-device-backed ext4 implementation requires a `no_std`-
//! compatible ext4 crate with a pluggable block-device abstraction.  At the
//! time of writing no such crate is available on crates.io with full
//! `no_std` support; the closest candidates (`ext4-view`, `ext2fs`) are
//! either read-only, not `no_std`, or do not expose a stable block-device
//! trait that maps cleanly onto the NVMe driver's interface.
//!
//! This module therefore provides a **stub** `Ext4Backend` that is backed by
//! the same in-memory storage as [`TmpfsBackend`].  The public API and
//! [`FsBackend`] implementation are complete and correct — only the
//! persistence layer is a placeholder.
//!
//! TODO: Replace the inner `TmpfsBackend` with a real ext4 block-device
//!       reader/writer once a suitable `no_std` crate (or an in-tree
//!       implementation) is available.  The wiring in `boot.rs` and
//!       `syscall/handler.rs` will not need to change.

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::fs::vfs::{
    DirEntry, FsBackend, FsError, InodeId, InodeStat, OpenFlags,
};
use crate::fs::tmpfs::TmpfsBackend;

// ---------------------------------------------------------------------------
// Ext4Backend
// ---------------------------------------------------------------------------

/// ext4 filesystem backend.
///
/// Currently backed by an in-memory [`TmpfsBackend`] (stub implementation).
/// All [`FsBackend`] methods are fully functional — data just isn't persisted
/// to a block device yet.
pub struct Ext4Backend {
    inner: Arc<TmpfsBackend>,
}

impl Ext4Backend {
    /// Create a new ext4 backend.
    ///
    /// In the stub implementation this creates a fresh in-memory store.
    /// A future block-device-backed implementation would accept a device
    /// identifier and mount the on-disk ext4 volume.
    pub fn new() -> Self {
        Ext4Backend {
            inner: Arc::new(TmpfsBackend::new()),
        }
    }

    /// Create an ext4 backend over an existing [`TmpfsBackend`] (for testing).
    #[cfg(test)]
    pub fn from_tmpfs(tmpfs: TmpfsBackend) -> Self {
        Ext4Backend {
            inner: Arc::new(tmpfs),
        }
    }
}

impl Default for Ext4Backend {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// FsBackend delegation to the inner TmpfsBackend
// ---------------------------------------------------------------------------

impl FsBackend for Ext4Backend {
    fn root_inode(&self) -> InodeId {
        self.inner.root_inode()
    }

    fn lookup(&self, parent: InodeId, name: &str) -> Result<InodeId, FsError> {
        self.inner.lookup(parent, name)
    }

    fn open(&self, inode: InodeId, flags: OpenFlags) -> Result<(), FsError> {
        self.inner.open(inode, flags)
    }

    fn read(&self, inode: InodeId, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        self.inner.read(inode, offset, buf)
    }

    fn write(&self, inode: InodeId, offset: u64, buf: &[u8]) -> Result<usize, FsError> {
        self.inner.write(inode, offset, buf)
    }

    fn stat(&self, inode: InodeId) -> Result<InodeStat, FsError> {
        self.inner.stat(inode)
    }

    fn readdir(&self, inode: InodeId) -> Result<Vec<DirEntry>, FsError> {
        self.inner.readdir(inode)
    }

    fn mkdir(&self, parent: InodeId, name: &str, mode: u32) -> Result<InodeId, FsError> {
        self.inner.mkdir(parent, name, mode)
    }

    fn unlink(&self, parent: InodeId, name: &str) -> Result<(), FsError> {
        self.inner.unlink(parent, name)
    }

    fn rename(
        &self,
        old_parent: InodeId,
        old_name: &str,
        new_parent: InodeId,
        new_name: &str,
    ) -> Result<(), FsError> {
        self.inner.rename(old_parent, old_name, new_parent, new_name)
    }

    fn sync(&self) -> Result<(), FsError> {
        // TODO: flush dirty blocks to the NVMe device when the block-device
        //       backend is implemented.
        self.inner.sync()
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::sync::Arc;
    use crate::fs::vfs::{FsBackend, MountFlags, Vfs};
    use crate::fs::tmpfs::TmpfsBackend;

    #[test]
    fn root_inode_is_one() {
        let ext4 = Ext4Backend::new();
        assert_eq!(ext4.root_inode(), InodeId(1));
    }

    #[test]
    fn mkdir_and_lookup() {
        let ext4 = Ext4Backend::new();
        let id = ext4.mkdir(InodeId(1), "subdir", 0o755).expect("mkdir");
        let found = ext4.lookup(InodeId(1), "subdir").expect("lookup");
        assert_eq!(found, id);
    }

    #[test]
    fn write_read_roundtrip() {
        let tmpfs = TmpfsBackend::new();
        let file_id = {
            let mut inner = tmpfs.inner.lock();
            inner.create_file(InodeId(1), "data.txt", 0o644).unwrap()
        };
        let ext4_from_tmpfs = Ext4Backend::from_tmpfs(tmpfs);

        ext4_from_tmpfs
            .write(file_id, 0, b"ext4 data")
            .expect("write");
        let mut buf = [0u8; 9];
        let n = ext4_from_tmpfs.read(file_id, 0, &mut buf).expect("read");
        assert_eq!(n, 9);
        assert_eq!(&buf, b"ext4 data");
    }

    #[test]
    fn sync_is_ok() {
        let ext4 = Ext4Backend::new();
        assert!(ext4.sync().is_ok());
    }

    #[test]
    fn mounts_at_slash_mnt_via_vfs() {
        let mut vfs = Vfs::new();
        let root: Arc<dyn FsBackend> = Arc::new(TmpfsBackend::new());
        let ext4: Arc<dyn FsBackend> = Arc::new(Ext4Backend::new());
        vfs.mount("/", root, MountFlags::default()).expect("mount /");
        vfs.mount("/mnt", ext4, MountFlags::default()).expect("mount /mnt");
        // Resolve /mnt — must go to the ext4 backend.
        let (entry, rel) = vfs.resolve("/mnt").expect("resolve /mnt");
        assert_eq!(entry.mount_point, "/mnt");
        assert_eq!(rel, "/");
    }
}
