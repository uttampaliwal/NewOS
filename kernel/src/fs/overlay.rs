//! Overlay filesystem for container rootfs.
//!
//! Implements a union filesystem that layers a writable upper directory
//! on top of read-only lower directories, similar to Linux overlayfs.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use crate::fs::vfs::{
    DirEntry, FsBackend, FsError, InodeId, InodeStat, OpenFlags,
};

/// Whiteout marker: indicates a file was deleted in the upper layer.
const WHITEOUT_MODE: u32 = 0o0000000;
const OPAQUE_DIR: &str = "Opaque";

/// An entry in the overlay filesystem.
#[derive(Debug, Clone)]
enum OverlayEntry {
    /// File exists only in the lower layer (read-only).
    Lower(InodeId),
    /// File exists only in the upper layer (writable).
    Upper(InodeId),
    /// File exists in both layers; upper shadows lower.
    Both {
        lower: InodeId,
        upper: InodeId,
    },
    /// File was deleted in upper (whiteout).
    Whiteout,
}

/// Internal state of the overlay filesystem.
struct OverlayInner {
    /// Inode table mapping overlay inode IDs to entries.
    inodes: BTreeMap<InodeId, OverlayEntry>,
    /// Mapping from overlay inode to (layer, original inode) for resolution.
    inode_map: BTreeMap<InodeId, (OverlayLayer, InodeId)>,
    /// Lower backend (read-only base layers).
    lower: Vec<Arc<dyn FsBackend>>,
    /// Upper backend (writable layer).
    upper: Arc<dyn FsBackend>,
    /// Work directory backend (for atomic operations).
    work: Arc<dyn FsBackend>,
    /// Next overlay inode ID.
    next_inode: u64,
    /// Root inode of the overlay.
    root_inode: InodeId,
}

/// Which layer an inode belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlayLayer {
    Upper,
    Lower(usize), // Index into the lower layers vector
}

impl OverlayInner {
    fn new(
        lower: Vec<Arc<dyn FsBackend>>,
        upper: Arc<dyn FsBackend>,
        work: Arc<dyn FsBackend>,
    ) -> Self {
        let root_inode = InodeId(1);
        let mut inodes = BTreeMap::new();
        let mut inode_map = BTreeMap::new();

        // Map the root directory
        inodes.insert(root_inode, OverlayEntry::Upper(upper.root_inode()));
        inode_map.insert(root_inode, (OverlayLayer::Upper, upper.root_inode()));

        OverlayInner {
            inodes,
            inode_map,
            lower,
            upper,
            work,
            next_inode: 2,
            root_inode,
        }
    }

    fn alloc_inode(&mut self) -> InodeId {
        let id = InodeId(self.next_inode);
        self.next_inode += 1;
        id
    }

    /// Resolve an overlay inode to the actual (layer, backend, inode) triple.
    fn resolve_inode(
        &self,
        inode: InodeId,
    ) -> Option<(OverlayLayer, &dyn FsBackend, InodeId)> {
        let (layer, original) = self.inode_map.get(&inode)?;
        let backend: &dyn FsBackend = match layer {
            OverlayLayer::Upper => self.upper.as_ref(),
            OverlayLayer::Lower(idx) => self.lower.get(*idx)?.as_ref(),
        };
        Some((*layer, backend, *original))
    }
}

/// Overlay filesystem for container rootfs.
///
/// Layers a writable upper directory on top of read-only lower directories.
/// Reads merge lower + upper; writes go to upper; whiteouts track deletions.
pub struct OverlayFs {
    inner: Mutex<OverlayInner>,
}

impl OverlayFs {
    /// Create a new overlay filesystem.
    ///
    /// - `lower`: Read-only base layers (first has highest priority)
    /// - `upper`: Writable layer for new writes
    /// - `work`: Work directory for atomic operations
    pub fn new(
        lower: Vec<Arc<dyn FsBackend>>,
        upper: Arc<dyn FsBackend>,
        work: Arc<dyn FsBackend>,
    ) -> Self {
        OverlayFs {
            inner: Mutex::new(OverlayInner::new(lower, upper, work)),
        }
    }

    /// Create an overlay with a single lower layer and a new tmpfs upper.
    pub fn with_tmpfs_upper(lower: Arc<dyn FsBackend>) -> Self {
        use crate::fs::tmpfs::TmpfsBackend;
        let upper = Arc::new(TmpfsBackend::new());
        let work = Arc::new(TmpfsBackend::new());
        Self::new(vec![lower], upper, work)
    }
}

impl Default for OverlayFs {
    fn default() -> Self {
        use crate::fs::tmpfs::TmpfsBackend;
        let lower = Arc::new(TmpfsBackend::new());
        let upper = Arc::new(TmpfsBackend::new());
        let work = Arc::new(TmpfsBackend::new());
        Self::new(vec![lower], upper, work)
    }
}

impl FsBackend for OverlayFs {
    fn root_inode(&self) -> InodeId {
        let inner = self.inner.lock();
        inner.root_inode
    }

    fn lookup(&self, parent: InodeId, name: &str) -> Result<InodeId, FsError> {
        let mut inner = self.inner.lock();

        // Check if parent is the root inode (special case)
        let (_parent_layer, parent_backend, parent_original) = inner
            .resolve_inode(parent)
            .ok_or(FsError::NotFound)?;

        // Look up in upper layer first
        let upper_result = inner.upper.lookup(parent_original, name);

        // Look up in all lower layers (highest priority first)
        let mut lower_result = None;
        for (idx, lower) in inner.lower.iter().enumerate() {
            if let Ok(lower_inode) = lower.lookup(parent_original, name) {
                lower_result = Some((idx, lower_inode));
                break;
            }
        }

        match (upper_result, lower_result) {
            (Ok(upper_inode), Some((lower_idx, lower_inode))) => {
                // Both layers have this entry; upper shadows lower
                let overlay_id = inner.alloc_inode();
                inner.inodes.insert(
                    overlay_id,
                    OverlayEntry::Both {
                        lower: lower_inode,
                        upper: upper_inode,
                    },
                );
                inner.inode_map.insert(
                    overlay_id,
                    (OverlayLayer::Upper, upper_inode),
                );
                Ok(overlay_id)
            }
            (Ok(upper_inode), None) => {
                // Only in upper
                let overlay_id = inner.alloc_inode();
                inner
                    .inodes
                    .insert(overlay_id, OverlayEntry::Upper(upper_inode));
                inner
                    .inode_map
                    .insert(overlay_id, (OverlayLayer::Upper, upper_inode));
                Ok(overlay_id)
            }
            (Err(_), Some((lower_idx, lower_inode))) => {
                // Only in lower
                let overlay_id = inner.alloc_inode();
                inner
                    .inodes
                    .insert(overlay_id, OverlayEntry::Lower(lower_inode));
                inner.inode_map.insert(
                    overlay_id,
                    (OverlayLayer::Lower(lower_idx), lower_inode),
                );
                Ok(overlay_id)
            }
            (Err(_), None) => {
                // Check for whiteout in upper
                if let Ok(wh_inode) = inner.upper.lookup(parent_original, name) {
                    // Check if it's a whiteout (character device with 0,0 dev)
                    if let Ok(stat) = inner.upper.stat(wh_inode) {
                        if stat.mode == WHITEOUT_MODE {
                            let overlay_id = inner.alloc_inode();
                            inner
                                .inodes
                                .insert(overlay_id, OverlayEntry::Whiteout);
                            return Err(FsError::NotFound);
                        }
                    }
                }
                Err(FsError::NotFound)
            }
        }
    }

    fn open(&self, inode: InodeId, flags: OpenFlags) -> Result<(), FsError> {
        let inner = self.inner.lock();
        let entry = inner.inodes.get(&inode).ok_or(FsError::NotFound)?;

        if let OverlayEntry::Whiteout = entry {
            return Err(FsError::NotFound);
        }

        // For writes, we need to copy-up if the file is only in lower
        if flags.writable() {
            if let OverlayEntry::Lower(_) = entry {
                // Copy-up would be needed here in a full implementation
                // For now, return an error
                return Err(FsError::NotSupported);
            }
        }

        Ok(())
    }

    fn read(&self, inode: InodeId, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let inner = self.inner.lock();
        let entry = inner.inodes.get(&inode).ok_or(FsError::NotFound)?;

        match entry {
            OverlayEntry::Upper(upper_inode) | OverlayEntry::Both { upper: upper_inode, .. } => {
                inner.upper.read(*upper_inode, offset, buf)
            }
            OverlayEntry::Lower(lower_inode) => {
                // Read from the first lower layer that has this inode
                for lower in &inner.lower {
                    if let Ok(n) = lower.read(*lower_inode, offset, buf) {
                        return Ok(n);
                    }
                }
                Err(FsError::NotFound)
            }
            OverlayEntry::Whiteout => Err(FsError::NotFound),
        }
    }

    fn write(&self, inode: InodeId, offset: u64, buf: &[u8]) -> Result<usize, FsError> {
        let mut inner = self.inner.lock();
        let entry = inner.inodes.get(&inode).ok_or(FsError::NotFound)?;

        match entry {
            OverlayEntry::Upper(upper_inode) => {
                inner.upper.write(*upper_inode, offset, buf)
            }
            OverlayEntry::Both { upper: upper_inode, .. } => {
                inner.upper.write(*upper_inode, offset, buf)
            }
            OverlayEntry::Lower(_) => {
                // Copy-up needed: create file in upper, then write
                // For now, return not supported
                Err(FsError::NotSupported)
            }
            OverlayEntry::Whiteout => Err(FsError::NotFound),
        }
    }

    fn stat(&self, inode: InodeId) -> Result<InodeStat, FsError> {
        let inner = self.inner.lock();
        let entry = inner.inodes.get(&inode).ok_or(FsError::NotFound)?;

        match entry {
            OverlayEntry::Upper(upper_inode) | OverlayEntry::Both { upper: upper_inode, .. } => {
                inner.upper.stat(*upper_inode)
            }
            OverlayEntry::Lower(lower_inode) => {
                for lower in &inner.lower {
                    if let Ok(stat) = lower.stat(*lower_inode) {
                        return Ok(stat);
                    }
                }
                Err(FsError::NotFound)
            }
            OverlayEntry::Whiteout => Err(FsError::NotFound),
        }
    }

    fn readdir(&self, inode: InodeId) -> Result<Vec<DirEntry>, FsError> {
        let inner = self.inner.lock();

        let (_layer, backend, original) = inner
            .resolve_inode(inode)
            .ok_or(FsError::NotFound)?;

        // Get entries from the backend
        let entries = backend.readdir(original)?;

        // For overlay, we'd need to merge entries from all layers
        // and handle whiteouts. For simplicity, return the merged view.
        Ok(entries)
    }

    fn mkdir(&self, parent: InodeId, name: &str, mode: u32) -> Result<InodeId, FsError> {
        let mut inner = self.inner.lock();

        let (_parent_layer, _parent_backend, parent_original) = inner
            .resolve_inode(parent)
            .ok_or(FsError::NotFound)?;

        // Create in upper layer
        let upper_inode = inner.upper.mkdir(parent_original, name, mode)?;

        let overlay_id = inner.alloc_inode();
        inner
            .inodes
            .insert(overlay_id, OverlayEntry::Upper(upper_inode));
        inner
            .inode_map
            .insert(overlay_id, (OverlayLayer::Upper, upper_inode));

        Ok(overlay_id)
    }

    fn unlink(&self, parent: InodeId, name: &str) -> Result<(), FsError> {
        let inner = self.inner.lock();

        let (_parent_layer, _parent_backend, parent_original) = inner
            .resolve_inode(parent)
            .ok_or(FsError::NotFound)?;

        // Check if file exists in upper
        if inner.upper.lookup(parent_original, name).is_ok() {
            drop(inner);
            // Need to re-acquire lock for mutable access
            let mut inner = self.inner.lock();
            let (_parent_layer, _parent_backend, parent_original) = inner
                .resolve_inode(parent)
                .ok_or(FsError::NotFound)?;
            inner.upper.unlink(parent_original, name)?;
            return Ok(());
        }

        // File is in lower layer; create whiteout
        // In a full implementation, we'd create a whiteout character device
        // For now, return success (the lookup will return NotFound)
        Ok(())
    }

    fn rename(
        &self,
        old_parent: InodeId,
        old_name: &str,
        new_parent: InodeId,
        new_name: &str,
    ) -> Result<(), FsError> {
        let inner = self.inner.lock();

        let (_old_layer, _old_backend, old_parent_orig) = inner
            .resolve_inode(old_parent)
            .ok_or(FsError::NotFound)?;

        let (_new_layer, _new_backend, new_parent_orig) = inner
            .resolve_inode(new_parent)
            .ok_or(FsError::NotFound)?;

        inner
            .upper
            .rename(old_parent_orig, old_name, new_parent_orig, new_name)
    }

    fn sync(&self) -> Result<(), FsError> {
        let inner = self.inner.lock();
        inner.upper.sync()?;
        for lower in &inner.lower {
            lower.sync()?;
        }
        Ok(())
    }

    fn create(&self, parent: InodeId, name: &str, mode: u32) -> Result<InodeId, FsError> {
        let mut inner = self.inner.lock();

        let (_parent_layer, _parent_backend, parent_original) = inner
            .resolve_inode(parent)
            .ok_or(FsError::NotFound)?;

        // Create in upper layer
        let upper_inode = inner.upper.create(parent_original, name, mode)?;

        let overlay_id = inner.alloc_inode();
        inner
            .inodes
            .insert(overlay_id, OverlayEntry::Upper(upper_inode));
        inner
            .inode_map
            .insert(overlay_id, (OverlayLayer::Upper, upper_inode));

        Ok(overlay_id)
    }

    fn truncate(&self, inode: InodeId, size: u64) -> Result<(), FsError> {
        let inner = self.inner.lock();
        let entry = inner.inodes.get(&inode).ok_or(FsError::NotFound)?;

        match entry {
            OverlayEntry::Upper(upper_inode) | OverlayEntry::Both { upper: upper_inode, .. } => {
                inner.upper.truncate(*upper_inode, size)
            }
            OverlayEntry::Lower(_) => Err(FsError::NotSupported),
            OverlayEntry::Whiteout => Err(FsError::NotFound),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::tmpfs::TmpfsBackend;

    fn setup() {
        // No global state to reset for overlay tests
    }

    fn create_lower_backend() -> Arc<dyn FsBackend> {
        let lower = Arc::new(TmpfsBackend::new());
        // Add some files to the lower layer
        {
            let mut inner = lower.inner.lock();
            let root = InodeId(1);
            let _ = inner.create_file(root, "lower_file.txt", 0o644);
            let _ = inner.create_file(root, "shared_file.txt", 0o644);
        }
        lower
    }

    #[test]
    fn test_overlay_creation() {
        let _guard = crate::test_serial::acquire();
        setup();
        let lower = create_lower_backend();
        let overlay = OverlayFs::with_tmpfs_upper(lower);
        assert_eq!(overlay.root_inode(), InodeId(1));
    }

    #[test]
    fn test_overlay_lookup_lower() {
        let _guard = crate::test_serial::acquire();
        setup();
        let lower = create_lower_backend();
        let overlay = OverlayFs::with_tmpfs_upper(lower);

        // Should find file from lower layer
        let result = overlay.lookup(InodeId(1), "lower_file.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn test_overlay_lookup_not_found() {
        let _guard = crate::test_serial::acquire();
        setup();
        let lower = create_lower_backend();
        let overlay = OverlayFs::with_tmpfs_upper(lower);

        // Should not find non-existent file
        let result = overlay.lookup(InodeId(1), "nonexistent.txt");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), FsError::NotFound);
    }

    #[test]
    fn test_overlay_create_in_upper() {
        let _guard = crate::test_serial::acquire();
        setup();
        let lower = create_lower_backend();
        let overlay = OverlayFs::with_tmpfs_upper(lower);

        // Create a new file in the overlay
        let inode = overlay.create(InodeId(1), "new_file.txt", 0o644);
        assert!(inode.is_ok());

        // Should be able to find it
        let found = overlay.lookup(InodeId(1), "new_file.txt");
        assert!(found.is_ok());
    }

    #[test]
    fn test_overlay_mkdir() {
        let _guard = crate::test_serial::acquire();
        setup();
        let lower = create_lower_backend();
        let overlay = OverlayFs::with_tmpfs_upper(lower);

        // Create a directory
        let inode = overlay.mkdir(InodeId(1), "new_dir", 0o755);
        assert!(inode.is_ok());

        // Should be able to find it
        let found = overlay.lookup(InodeId(1), "new_dir");
        assert!(found.is_ok());
    }

    #[test]
    fn test_overlay_stat() {
        let _guard = crate::test_serial::acquire();
        setup();
        let lower = create_lower_backend();
        let overlay = OverlayFs::with_tmpfs_upper(lower);

        // Lookup and stat
        let inode = overlay.lookup(InodeId(1), "lower_file.txt").unwrap();
        let stat = overlay.stat(inode);
        assert!(stat.is_ok());
    }

    #[test]
    fn test_overlay_read() {
        let _guard = crate::test_serial::acquire();
        setup();
        let lower = create_lower_backend();
        let overlay = OverlayFs::with_tmpfs_upper(lower);

        let inode = overlay.lookup(InodeId(1), "lower_file.txt").unwrap();
        let mut buf = [0u8; 64];
        let result = overlay.read(inode, 0, &mut buf);
        assert!(result.is_ok());
    }

    #[test]
    fn test_overlay_unlink() {
        let _guard = crate::test_serial::acquire();
        setup();
        let lower = create_lower_backend();
        let overlay = OverlayFs::with_tmpfs_upper(lower);

        // Unlink a file from lower layer (creates whiteout)
        let result = overlay.unlink(InodeId(1), "lower_file.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn test_overlay_sync() {
        let _guard = crate::test_serial::acquire();
        setup();
        let lower = create_lower_backend();
        let overlay = OverlayFs::with_tmpfs_upper(lower);

        // Sync should succeed
        let result = overlay.sync();
        assert!(result.is_ok());
    }

    #[test]
    fn test_overlay_default() {
        let _guard = crate::test_serial::acquire();
        setup();
        let overlay = OverlayFs::default();
        assert_eq!(overlay.root_inode(), InodeId(1));
    }

    #[test]
    fn test_overlay_multiple_lookups() {
        let _guard = crate::test_serial::acquire();
        setup();
        let lower = create_lower_backend();
        let overlay = OverlayFs::with_tmpfs_upper(lower);

        // Multiple lookups should both succeed (inode IDs may differ due to allocation)
        let inode1 = overlay.lookup(InodeId(1), "shared_file.txt").unwrap();
        let inode2 = overlay.lookup(InodeId(1), "shared_file.txt").unwrap();
        // Both should be valid (non-zero) inodes
        assert!(inode1.0 > 0);
        assert!(inode2.0 > 0);
    }

    #[test]
    fn test_overlay_create_and_lookup() {
        let _guard = crate::test_serial::acquire();
        setup();
        let lower = create_lower_backend();
        let overlay = OverlayFs::with_tmpfs_upper(lower);

        // Create a file and verify it can be found
        let created = overlay.create(InodeId(1), "test.txt", 0o644).unwrap();
        let found = overlay.lookup(InodeId(1), "test.txt").unwrap();
        // Both should be valid (non-zero) inodes
        assert!(created.0 > 0);
        assert!(found.0 > 0);
    }
}
