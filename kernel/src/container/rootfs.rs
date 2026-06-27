//! Container rootfs setup.
//!
//! Provides functions to set up the root filesystem for containers,
//! including overlay filesystem support for layered images.

extern crate alloc;

use alloc::sync::Arc;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use crate::fs::vfs::{FsBackend, InodeId};
use crate::fs::overlay::OverlayFs;
use crate::fs::tmpfs::TmpfsBackend;

use super::spec::OciSpec;

/// Container rootfs errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootfsError {
    /// The rootfs path does not exist.
    PathNotFound(String),
    /// Failed to create overlay filesystem.
    OverlayFailed(String),
    /// Failed to mount a filesystem.
    MountFailed(String),
    /// The container spec is invalid.
    InvalidSpec(String),
    /// An I/O error occurred.
    IoError,
}

/// Container rootfs state.
pub struct ContainerRootfs {
    /// The overlay filesystem for this container's root.
    pub overlay: Arc<OverlayFs>,
    /// The merged root path (where the overlay is mounted).
    pub root_path: String,
    /// Whether the rootfs is read-only.
    pub readonly: bool,
    /// Layer paths (lower + upper).
    pub layer_paths: Vec<String>,
}

impl ContainerRootfs {
    /// Create a new container rootfs from an OCI spec.
    ///
    /// Sets up an overlay filesystem with the specified layers.
    pub fn from_spec(spec: &OciSpec) -> Result<Self, RootfsError> {
        let root_path = spec.root.path.clone();
        let readonly = spec.root.readonly;

        // Create the upper (writable) layer
        let upper = Arc::new(TmpfsBackend::new());

        // Create the work directory (for atomic operations)
        let work = Arc::new(TmpfsBackend::new());

        // Create lower layers from diff_ids
        let mut lower_layers: Vec<Arc<dyn FsBackend>> = Vec::new();
        let mut layer_paths = Vec::new();

        // Add a base lower layer if specified
        if !spec.root.diff_ids.is_empty() {
            // In a real implementation, we'd unpack layers from the image.
            // For now, create a tmpfs for each layer as a placeholder.
            for diff_id in &spec.root.diff_ids {
                let layer = Arc::new(TmpfsBackend::new());
                lower_layers.push(layer);
                layer_paths.push(diff_id.clone());
            }
        } else {
            // Create a default lower layer
            let lower = Arc::new(TmpfsBackend::new());
            lower_layers.push(lower);
            layer_paths.push(String::from("base"));
        }

        // Create the overlay filesystem
        let overlay = Arc::new(OverlayFs::new(lower_layers, upper, work));

        layer_paths.push(String::from("upper"));

        Ok(ContainerRootfs {
            overlay,
            root_path,
            readonly,
            layer_paths,
        })
    }

    /// Create a container rootfs with a single tmpfs layer (no overlay).
    pub fn with_single_layer(path: &str) -> Result<Self, RootfsError> {
        let overlay = Arc::new(OverlayFs::default());

        Ok(ContainerRootfs {
            overlay,
            root_path: String::from(path),
            readonly: false,
            layer_paths: vec![String::from("tmpfs")],
        })
    }

    /// Look up a file in the container rootfs.
    pub fn lookup(&self, path: &str) -> Result<InodeId, RootfsError> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current = self.overlay.root_inode();

        for part in parts {
            current = self.overlay.lookup(current, part)
                .map_err(|_| RootfsError::PathNotFound(path.to_string()))?;
        }

        Ok(current)
    }

    /// Read a file from the container rootfs.
    pub fn read(&self, inode: InodeId, offset: u64, buf: &mut [u8]) -> Result<usize, RootfsError> {
        self.overlay.read(inode, offset, buf)
            .map_err(|_| RootfsError::IoError)
    }

    /// Write a file to the container rootfs.
    pub fn write(&self, inode: InodeId, offset: u64, buf: &[u8]) -> Result<usize, RootfsError> {
        self.overlay.write(inode, offset, buf)
            .map_err(|_| RootfsError::IoError)
    }

    /// Create a directory in the container rootfs.
    pub fn mkdir(&self, path: &str, mode: u32) -> Result<InodeId, RootfsError> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(RootfsError::InvalidSpec("empty path".into()));
        }

        let (parent_path, dir_name) = parts.split_at(parts.len() - 1);
        let dir_name = dir_name[0];

        let mut current = self.overlay.root_inode();
        for part in parent_path {
            current = self.overlay.lookup(current, part)
                .map_err(|_| RootfsError::PathNotFound(path.to_string()))?;
        }

        self.overlay.mkdir(current, dir_name, mode)
            .map_err(|_| RootfsError::MountFailed("mkdir failed".into()))
    }

    /// Create a file in the container rootfs.
    pub fn create_file(&self, path: &str, mode: u32) -> Result<InodeId, RootfsError> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(RootfsError::InvalidSpec("empty path".into()));
        }

        let (parent_path, file_name) = parts.split_at(parts.len() - 1);
        let file_name = file_name[0];

        let mut current = self.overlay.root_inode();
        for part in parent_path {
            current = self.overlay.lookup(current, part)
                .map_err(|_| RootfsError::PathNotFound(path.to_string()))?;
        }

        self.overlay.create(current, file_name, mode)
            .map_err(|_| RootfsError::MountFailed("create failed".into()))
    }

    /// Set up standard container directories (/proc, /sys, /dev, etc.).
    pub fn setup_standard_dirs(&self) -> Result<(), RootfsError> {
        let dirs = ["/proc", "/sys", "/dev", "/dev/pts", "/dev/shm", "/tmp", "/var", "/var/run"];
        for dir in &dirs {
            if self.lookup(dir).is_err() {
                let _ = self.mkdir(dir, 0o755);
            }
        }
        Ok(())
    }

    /// Mount standard container filesystems.
    ///
    /// In a full implementation, this would mount procfs, sysfs, devtmpfs, etc.
    /// For now, this is a placeholder that creates the directories.
    pub fn mount_standard_filesystems(&self) -> Result<(), RootfsError> {
        // In a real implementation, we'd mount:
        // - procfs at /proc
        // - sysfs at /sys
        // - devtmpfs at /dev
        // - pts at /dev/pts
        // - shm at /dev/shm
        //
        // For now, just ensure the directories exist
        self.setup_standard_dirs()
    }
}

/// Global container rootfs manager.
pub struct ContainerRootfsManager {
    rootfses: BTreeMap<String, Arc<ContainerRootfs>>,
}

use alloc::collections::BTreeMap;

impl ContainerRootfsManager {
    /// Create a new rootfs manager.
    pub fn new() -> Self {
        ContainerRootfsManager {
            rootfses: BTreeMap::new(),
        }
    }

    /// Create a rootfs for a container.
    pub fn create_rootfs(
        &mut self,
        container_id: &str,
        spec: &OciSpec,
    ) -> Result<Arc<ContainerRootfs>, RootfsError> {
        let rootfs = ContainerRootfs::from_spec(spec)?;
        let rootfs = Arc::new(rootfs);

        // Set up standard directories
        rootfs.mount_standard_filesystems()?;

        self.rootfses.insert(String::from(container_id), rootfs.clone());
        Ok(rootfs)
    }

    /// Get a container's rootfs.
    pub fn get_rootfs(&self, container_id: &str) -> Option<Arc<ContainerRootfs>> {
        self.rootfses.get(container_id).cloned()
    }

    /// Remove a container's rootfs.
    pub fn remove_rootfs(&mut self, container_id: &str) -> Option<Arc<ContainerRootfs>> {
        self.rootfses.remove(container_id)
    }

    /// List all container rootfses.
    pub fn list_rootfses(&self) -> Vec<String> {
        self.rootfses.keys().cloned().collect()
    }
}

impl Default for ContainerRootfsManager {
    fn default() -> Self {
        Self::new()
    }
}

lazy_static::lazy_static! {
    /// Global container rootfs manager.
    pub static ref CONTAINER_ROOTFS_MANAGER: Mutex<ContainerRootfsManager> =
        Mutex::new(ContainerRootfsManager::new());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::spec::parse_default_spec;

    fn setup() {
        let mut manager = CONTAINER_ROOTFS_MANAGER.lock();
        *manager = ContainerRootfsManager::new();
    }

    #[test]
    fn test_rootfs_from_spec() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = parse_default_spec("/rootfs", &["/bin/sh"]);
        let rootfs = ContainerRootfs::from_spec(&spec);
        assert!(rootfs.is_ok());
        let rootfs = rootfs.unwrap();
        assert_eq!(rootfs.root_path, "/rootfs");
        assert!(!rootfs.readonly);
    }

    #[test]
    fn test_rootfs_single_layer() {
        let _guard = crate::test_serial::acquire();
        setup();
        let rootfs = ContainerRootfs::with_single_layer("/container").unwrap();
        assert_eq!(rootfs.root_path, "/container");
        assert!(!rootfs.readonly);
    }

    #[test]
    fn test_rootfs_lookup() {
        let _guard = crate::test_serial::acquire();
        setup();
        let rootfs = ContainerRootfs::with_single_layer("/").unwrap();
        // Lookup root should succeed
        let result = rootfs.lookup("/");
        assert!(result.is_ok());
    }

    #[test]
    fn test_rootfs_lookup_not_found() {
        let _guard = crate::test_serial::acquire();
        setup();
        let rootfs = ContainerRootfs::with_single_layer("/").unwrap();
        let result = rootfs.lookup("/nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_rootfs_mkdir() {
        let _guard = crate::test_serial::acquire();
        setup();
        let rootfs = ContainerRootfs::with_single_layer("/").unwrap();
        let result = rootfs.mkdir("/testdir", 0o755);
        assert!(result.is_ok());

        // Should be able to find it
        let found = rootfs.lookup("/testdir");
        assert!(found.is_ok());
    }

    #[test]
    fn test_rootfs_create_file() {
        let _guard = crate::test_serial::acquire();
        setup();
        let rootfs = ContainerRootfs::with_single_layer("/").unwrap();
        let result = rootfs.create_file("/testfile.txt", 0o644);
        assert!(result.is_ok());

        // Should be able to find it
        let found = rootfs.lookup("/testfile.txt");
        assert!(found.is_ok());
    }

    #[test]
    fn test_rootfs_setup_standard_dirs() {
        let _guard = crate::test_serial::acquire();
        setup();
        let rootfs = ContainerRootfs::with_single_layer("/").unwrap();
        let result = rootfs.setup_standard_dirs();
        assert!(result.is_ok());

        // Verify directories were created
        assert!(rootfs.lookup("/proc").is_ok());
        assert!(rootfs.lookup("/sys").is_ok());
        assert!(rootfs.lookup("/dev").is_ok());
    }

    #[test]
    fn test_rootfs_manager_create() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = parse_default_spec("/rootfs", &["/bin/sh"]);
        let mut manager = CONTAINER_ROOTFS_MANAGER.lock();
        let result = manager.create_rootfs("test-container", &spec);
        assert!(result.is_ok());
    }

    #[test]
    fn test_rootfs_manager_get() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = parse_default_spec("/rootfs", &["/bin/sh"]);
        {
            let mut manager = CONTAINER_ROOTFS_MANAGER.lock();
            manager.create_rootfs("test-container", &spec).unwrap();
        }

        let manager = CONTAINER_ROOTFS_MANAGER.lock();
        let rootfs = manager.get_rootfs("test-container");
        assert!(rootfs.is_some());
    }

    #[test]
    fn test_rootfs_manager_remove() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = parse_default_spec("/rootfs", &["/bin/sh"]);
        {
            let mut manager = CONTAINER_ROOTFS_MANAGER.lock();
            manager.create_rootfs("test-container", &spec).unwrap();
        }

        let mut manager = CONTAINER_ROOTFS_MANAGER.lock();
        let removed = manager.remove_rootfs("test-container");
        assert!(removed.is_some());
        assert!(manager.get_rootfs("test-container").is_none());
    }

    #[test]
    fn test_rootfs_manager_list() {
        let _guard = crate::test_serial::acquire();
        setup();
        let spec = parse_default_spec("/rootfs", &["/bin/sh"]);
        {
            let mut manager = CONTAINER_ROOTFS_MANAGER.lock();
            manager.create_rootfs("container-1", &spec).unwrap();
            manager.create_rootfs("container-2", &spec).unwrap();
        }

        let manager = CONTAINER_ROOTFS_MANAGER.lock();
        let list = manager.list_rootfses();
        assert_eq!(list.len(), 2);
        assert!(list.contains(&String::from("container-1")));
        assert!(list.contains(&String::from("container-2")));
    }

    #[test]
    fn test_rootfs_readonly_spec() {
        let _guard = crate::test_serial::acquire();
        setup();
        let mut spec = parse_default_spec("/rootfs", &["/bin/sh"]);
        spec.root.readonly = true;
        let rootfs = ContainerRootfs::from_spec(&spec).unwrap();
        assert!(rootfs.readonly);
    }

    #[test]
    fn test_rootfs_with_layers() {
        let _guard = crate::test_serial::acquire();
        setup();
        let mut spec = parse_default_spec("/rootfs", &["/bin/sh"]);
        spec.root.diff_ids = vec![
            String::from("sha256:layer1"),
            String::from("sha256:layer2"),
        ];
        let rootfs = ContainerRootfs::from_spec(&spec).unwrap();
        assert_eq!(rootfs.layer_paths.len(), 3); // 2 layers + upper
    }
}
