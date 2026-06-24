use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// SnapshotId
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SnapshotId(String);

impl SnapshotId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn generate() -> Self {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self(format!("snap-{nanos:x}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// SnapshotTrigger
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotTrigger {
    PreInstall,
    PreRemove,
    PreUpgrade,
    Manual(String),
}

impl std::fmt::Display for SnapshotTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotTrigger::PreInstall => write!(f, "pre-install"),
            SnapshotTrigger::PreRemove => write!(f, "pre-remove"),
            SnapshotTrigger::PreUpgrade => write!(f, "pre-upgrade"),
            SnapshotTrigger::Manual(desc) => write!(f, "manual({desc})"),
        }
    }
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: SnapshotId,
    pub trigger: SnapshotTrigger,
    pub timestamp: SystemTime,
    pub paths: Vec<PathBuf>,
}

impl Snapshot {
    pub fn new(trigger: SnapshotTrigger, paths: Vec<PathBuf>) -> Self {
        Self {
            id: SnapshotId::generate(),
            trigger,
            timestamp: SystemTime::now(),
            paths,
        }
    }
}

// ---------------------------------------------------------------------------
// SnapshotError
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum SnapshotError {
    IoError(String),
    NotFound(SnapshotId),
    RollbackFailed(String),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::IoError(msg) => write!(f, "I/O error: {msg}"),
            SnapshotError::NotFound(id) => write!(f, "snapshot not found: {id}"),
            SnapshotError::RollbackFailed(msg) => write!(f, "rollback failed: {msg}"),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl From<std::io::Error> for SnapshotError {
    fn from(e: std::io::Error) -> Self {
        SnapshotError::IoError(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// SnapshotManager
// ---------------------------------------------------------------------------

pub struct SnapshotManager {
    snapshot_dir: PathBuf,
    root: PathBuf,
}

impl SnapshotManager {
    pub fn new(snapshot_dir: PathBuf) -> Self {
        Self {
            snapshot_dir,
            root: PathBuf::from("/"),
        }
    }

    pub fn with_root(snapshot_dir: PathBuf, root: PathBuf) -> Self {
        Self { snapshot_dir, root }
    }

    pub fn snapshot_dir(&self) -> &Path {
        &self.snapshot_dir
    }

    /// Create a snapshot by copying the listed paths into the snapshot
    /// directory.  Each path's relative structure is preserved under the
    /// snapshot id.
    pub fn create_snapshot(
        &self,
        trigger: SnapshotTrigger,
        paths: &[PathBuf],
    ) -> Result<Snapshot, SnapshotError> {
        let snap = Snapshot::new(trigger, paths.to_vec());
        let snap_path = self.snapshot_dir.join(snap.id.as_str());
        fs::create_dir_all(&snap_path)?;

        let mut relative_paths: Vec<String> = Vec::new();
        for path in &snap.paths {
            if path.exists() {
                let relative = if path.is_absolute() {
                    path.strip_prefix(&self.root)
                        .unwrap_or_else(|_| path.strip_prefix("/").unwrap_or(path))
                        .to_path_buf()
                } else {
                    path.clone()
                };
                relative_paths.push(relative.to_string_lossy().to_string());
                let dest = snap_path.join(&relative);
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                if path.is_file() {
                    fs::copy(path, &dest)?;
                } else if path.is_dir() {
                    copy_dir_recursive(path, &dest)?;
                }
            }
        }

        let manifest_path = snap_path.join(".snapshot.json");
        let manifest = serde_json::json!({
            "id": snap.id.as_str(),
            "trigger": format!("{}", snap.trigger),
            "timestamp_nanos": snap.timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            "paths": relative_paths,
        });
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|e| SnapshotError::IoError(e.to_string()))?;
        fs::write(&manifest_path, manifest_bytes)?;

        Ok(snap)
    }

    /// Roll back to a previously created snapshot by restoring every path
    /// from the snapshot directory.
    pub fn rollback(&self, id: &SnapshotId) -> Result<(), SnapshotError> {
        let snap_path = self.snapshot_dir.join(id.as_str());
        if !snap_path.exists() {
            return Err(SnapshotError::NotFound(id.clone()));
        }

        let manifest_path = snap_path.join(".snapshot.json");
        let manifest_bytes = fs::read_to_string(&manifest_path)
            .map_err(|e| SnapshotError::IoError(e.to_string()))?;
        let manifest: serde_json::Value = serde_json::from_str(&manifest_bytes)
            .map_err(|e| SnapshotError::IoError(e.to_string()))?;

        let paths = manifest["paths"]
            .as_array()
            .ok_or_else(|| SnapshotError::RollbackFailed("corrupt snapshot manifest".into()))?;

        for path_val in paths {
            let relative = path_val.as_str().ok_or_else(|| {
                SnapshotError::RollbackFailed("corrupt path in snapshot manifest".into())
            })?;
            let src = snap_path.join(relative);
            let dest = self.root.join(relative);

            if src.is_file() {
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&src, &dest)?;
            } else if src.is_dir() {
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                copy_dir_recursive(&src, &dest)?;
            }
        }

        Ok(())
    }

    /// List all snapshots stored in the snapshot directory.
    pub fn list_snapshots(&self) -> Result<Vec<SnapshotId>, SnapshotError> {
        let mut snapshots = Vec::new();
        if !self.snapshot_dir.exists() {
            return Ok(snapshots);
        }
        for entry in fs::read_dir(&self.snapshot_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if entry.file_type()?.is_dir() && name_str.starts_with("snap-") {
                snapshots.push(SnapshotId::new(name_str.into_owned()));
            }
        }
        snapshots.sort();
        Ok(snapshots)
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), SnapshotError> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// PackageDb — simple JSON-based package database
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageEntry {
    pub name: String,
    pub version: String,
    pub files: Vec<String>,
    pub installed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackageDb {
    pub packages: BTreeMap<String, PackageEntry>,
}

impl PackageDb {
    pub fn load(path: &Path) -> Result<Self, SnapshotError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = fs::read(path)?;
        let db: PackageDb =
            serde_json::from_slice(&bytes).map_err(|e| SnapshotError::IoError(e.to_string()))?;
        Ok(db)
    }

    pub fn save(&self, path: &Path) -> Result<(), SnapshotError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes =
            serde_json::to_vec_pretty(self).map_err(|e| SnapshotError::IoError(e.to_string()))?;
        fs::write(path, bytes)?;
        Ok(())
    }

    pub fn is_installed(&self, name: &str) -> bool {
        self.packages.contains_key(name)
    }

    pub fn add_package(&mut self, entry: PackageEntry) {
        self.packages.insert(entry.name.clone(), entry);
    }

    pub fn remove_package(&mut self, name: &str) {
        self.packages.remove(name);
    }

    pub fn get(&self, name: &str) -> Option<&PackageEntry> {
        self.packages.get(name)
    }

    pub fn all_packages(&self) -> impl Iterator<Item = &PackageEntry> {
        self.packages.values()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manager() -> (SnapshotManager, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let snap_dir = tmp.path().join("snapshots");
        let mgr = SnapshotManager::with_root(snap_dir, tmp.path().to_path_buf());
        (mgr, tmp)
    }

    #[test]
    fn test_create_and_list_snapshots() {
        let (mgr, _tmp) = test_manager();
        let test_file = _tmp.path().join("test.txt");
        fs::write(&test_file, b"hello").unwrap();

        let snap = mgr
            .create_snapshot(SnapshotTrigger::Manual("test".into()), &[test_file])
            .unwrap();
        assert!(snap.id.as_str().starts_with("snap-"));

        let list = mgr.list_snapshots().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0], snap.id);
    }

    #[test]
    fn test_rollback_restores_files() {
        let (mgr, _tmp) = test_manager();
        let target = _tmp.path().join("target.txt");
        fs::write(&target, b"original").unwrap();

        // Create snapshot of the original file
        let snap = mgr
            .create_snapshot(SnapshotTrigger::PreInstall, std::slice::from_ref(&target))
            .unwrap();

        // Modify the file
        fs::write(&target, b"modified").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "modified");

        // Rollback
        mgr.rollback(&snap.id).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "original");
    }

    #[test]
    fn test_rollback_nonexistent_snapshot() {
        let (mgr, _tmp) = test_manager();
        let id = SnapshotId::new("snap-nonexistent");
        let result = mgr.rollback(&id);
        assert!(matches!(result, Err(SnapshotError::NotFound(_))));
    }

    #[test]
    fn test_snapshot_id_generate_unique() {
        let a = SnapshotId::generate();
        let b = SnapshotId::generate();
        assert_ne!(a, b);
    }

    #[test]
    fn test_snapshot_error_display() {
        let err = SnapshotError::NotFound(SnapshotId::new("snap-1"));
        assert!(!format!("{err}").is_empty());

        let err = SnapshotError::IoError("disk full".into());
        assert!(format!("{err}").contains("disk full"));

        let err = SnapshotError::RollbackFailed("corrupt".into());
        assert!(format!("{err}").contains("corrupt"));
    }

    // -----------------------------------------------------------------------
    // PackageDb tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_package_db_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("packages.json");
        let db = PackageDb::load(&db_path).unwrap();
        assert!(db.all_packages().next().is_none());
    }

    #[test]
    fn test_package_db_add_and_list() {
        let mut db = PackageDb::default();
        db.add_package(PackageEntry {
            name: "hello".into(),
            version: "1.0.0".into(),
            files: vec!["/usr/bin/hello".into()],
            installed_at: 1000,
        });
        assert!(db.is_installed("hello"));
        assert!(!db.is_installed("nonexistent"));
        assert_eq!(db.all_packages().count(), 1);
    }

    #[test]
    fn test_package_db_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("packages.json");

        let mut db = PackageDb::default();
        db.add_package(PackageEntry {
            name: "foo".into(),
            version: "2.0.0".into(),
            files: vec!["/usr/bin/foo".into()],
            installed_at: 2000,
        });
        db.save(&db_path).unwrap();

        let loaded = PackageDb::load(&db_path).unwrap();
        assert!(loaded.is_installed("foo"));
        assert_eq!(loaded.get("foo").unwrap().version, "2.0.0");
    }

    #[test]
    fn test_package_db_remove() {
        let mut db = PackageDb::default();
        db.add_package(PackageEntry {
            name: "a".into(),
            version: "1.0".into(),
            files: vec![],
            installed_at: 0,
        });
        assert!(db.is_installed("a"));
        db.remove_package("a");
        assert!(!db.is_installed("a"));
    }
}
