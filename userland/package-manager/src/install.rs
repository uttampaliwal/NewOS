use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::fetcher::{FetchError, PackageFetcher, verify_sha256};
use crate::snapshot::{
    PackageDb, PackageEntry, Snapshot, SnapshotError, SnapshotId, SnapshotManager, SnapshotTrigger,
};
use semver::Version;
use tpkg_format::{PackageSource, ResolvedPackage, TpkgManifest};

// ---------------------------------------------------------------------------
// InstallError
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum InstallError {
    SnapshotError(SnapshotError),
    FetchError(FetchError),
    IoError(String),
    VerificationFailed(String),
    PackageConflict(String),
    PackageNotFound(String),
    RollbackError(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::SnapshotError(e) => write!(f, "snapshot error: {e}"),
            InstallError::FetchError(e) => write!(f, "fetch error: {e}"),
            InstallError::IoError(msg) => write!(f, "I/O error: {msg}"),
            InstallError::VerificationFailed(msg) => write!(f, "verification failed: {msg}"),
            InstallError::PackageConflict(name) => {
                write!(f, "package conflict: {name} is already installed")
            }
            InstallError::PackageNotFound(name) => write!(f, "package not found: {name}"),
            InstallError::RollbackError(msg) => write!(f, "rollback error: {msg}"),
        }
    }
}

impl std::error::Error for InstallError {}

impl From<SnapshotError> for InstallError {
    fn from(e: SnapshotError) -> Self {
        InstallError::SnapshotError(e)
    }
}

impl From<FetchError> for InstallError {
    fn from(e: FetchError) -> Self {
        InstallError::FetchError(e)
    }
}

// ---------------------------------------------------------------------------
// InstallPipeline
// ---------------------------------------------------------------------------

/// The install pipeline manages the full lifecycle of package operations:
///
/// 1. Create a pre-install snapshot
/// 2. Fetch the package archive (TUF-verified)
/// 3. Verify checksum against manifest
/// 4. Extract to staging directory
/// 5. Apply to the real filesystem
/// 6. Update the package database
///
/// On any failure after step 1, the pipeline automatically rolls back
/// the filesystem to the pre-install state.
pub struct InstallPipeline {
    fetcher: PackageFetcher,
    snapshot_mgr: SnapshotManager,
    db_path: PathBuf,
    staging_dir: PathBuf,
    target_root: PathBuf,
}

impl InstallPipeline {
    pub fn new(
        fetcher: PackageFetcher,
        snapshot_mgr: SnapshotManager,
        db_path: PathBuf,
        staging_dir: PathBuf,
    ) -> Self {
        Self {
            fetcher,
            snapshot_mgr,
            db_path,
            staging_dir,
            target_root: PathBuf::from("/"),
        }
    }

    /// Install a resolved package.
    ///
    /// Returns the `SnapshotId` of the pre-install snapshot so callers can
    /// examine it if needed.
    pub async fn install(&self, resolved: &ResolvedPackage) -> Result<SnapshotId, InstallError> {
        let mut db = PackageDb::load(&self.db_path)?;

        if db.is_installed(resolved.name.as_str()) {
            return Err(InstallError::PackageConflict(resolved.name.to_string()));
        }

        let install_paths = self.collect_install_paths(resolved);

        // Step 1: Pre-install snapshot
        let snapshot = self
            .snapshot_mgr
            .create_snapshot(SnapshotTrigger::PreInstall, &install_paths)?;
        let snap_id = snapshot.id.clone();

        // Steps 2-6 with automatic rollback on failure
        match self.run_install(resolved, &snapshot, &mut db).await {
            Ok(()) => Ok(snap_id),
            Err(e) => {
                // Rollback on any failure
                if let Err(rollback_err) = self.snapshot_mgr.rollback(&snap_id) {
                    return Err(InstallError::RollbackError(format!(
                        "install failed ({e}), then rollback failed: {rollback_err}"
                    )));
                }
                Err(e)
            }
        }
    }

    async fn run_install(
        &self,
        resolved: &ResolvedPackage,
        _snapshot: &Snapshot,
        db: &mut PackageDb,
    ) -> Result<(), InstallError> {
        // Step 2: Fetch the package archive
        let archive_path = self.fetcher.fetch(resolved).await?;

        // Step 3: Verify checksum
        self.verify_archive(&archive_path, resolved)?;

        // Step 4: Extract to staging
        let staging_pkg = self.staging_dir.join(format!("{}-{}", resolved.name, resolved.version));
        if staging_pkg.exists() {
            fs::remove_dir_all(&staging_pkg)
                .map_err(|e| InstallError::IoError(format!("cannot clean staging: {e}")))?;
        }
        extract_archive(&archive_path, &staging_pkg)?;

        // Step 5: Apply to filesystem
        self.apply_to_filesystem(&staging_pkg, resolved)?;

        // Step 6: Update package database
        let installed_files = self.collect_files_in_dir(&staging_pkg);
        let entry = PackageEntry {
            name: resolved.name.to_string(),
            version: resolved.version.to_string(),
            files: installed_files,
            installed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };
        db.add_package(entry);
        db.save(&self.db_path)?;

        Ok(())
    }

    /// Remove an installed package.
    ///
    /// Creates a pre-remove snapshot, removes files tracked in the database,
    /// and updates the database.
    pub fn remove(&self, package_name: &str) -> Result<SnapshotId, InstallError> {
        let mut db = PackageDb::load(&self.db_path)?;
        let entry = db
            .get(package_name)
            .ok_or_else(|| InstallError::PackageNotFound(package_name.to_string()))?;

        let paths: Vec<PathBuf> = entry
            .files
            .iter()
            .map(|f| self.target_root.join(f.strip_prefix("/").unwrap_or(f)))
            .collect();

        // Pre-remove snapshot
        let snapshot = self
            .snapshot_mgr
            .create_snapshot(SnapshotTrigger::PreRemove, &paths)?;
        let snap_id = snapshot.id.clone();

        // Remove files
        for path in &paths {
            if path.exists() {
                if path.is_dir() {
                    let _ = fs::remove_dir_all(path);
                } else {
                    let _ = fs::remove_file(path);
                }
            }
        }

        db.remove_package(package_name);
        db.save(&self.db_path)
            .map_err(|e| InstallError::RollbackError(format!("db save failed: {e}")))?;

        Ok(snap_id)
    }

    /// Upgrade a package to a new version.
    ///
    /// Creates a pre-upgrade snapshot, removes old files, installs new ones.
    pub async fn upgrade(&self, resolved: &ResolvedPackage) -> Result<SnapshotId, InstallError> {
        let mut db = PackageDb::load(&self.db_path)?;
        let name = resolved.name.as_str();

        if !db.is_installed(name) {
            // Not installed — treat as a fresh install
            return self.install(resolved).await;
        }

        let old_entry = db.get(name).cloned().unwrap();
        let old_paths: Vec<PathBuf> = old_entry
            .files
            .iter()
            .map(|f| self.target_root.join(f.strip_prefix("/").unwrap_or(f)))
            .collect();

        // Pre-upgrade snapshot that captures both old files and new install paths
        let mut all_paths = old_paths.clone();
        all_paths.extend(self.collect_install_paths(resolved));
        let snapshot = self
            .snapshot_mgr
            .create_snapshot(SnapshotTrigger::PreUpgrade, &all_paths)?;
        let snap_id = snapshot.id.clone();

        match self.run_install(resolved, &snapshot, &mut db).await {
            Ok(()) => {
                // Remove old files that are not part of the new package
                let new_files: Vec<PathBuf> = self.collect_install_paths(resolved);
                for old_path in &old_paths {
                    if !new_files.contains(old_path) && old_path.exists() {
                        if old_path.is_dir() {
                            let _ = fs::remove_dir_all(old_path);
                        } else {
                            let _ = fs::remove_file(old_path);
                        }
                    }
                }
                Ok(snap_id)
            }
            Err(e) => {
                if let Err(rollback_err) = self.snapshot_mgr.rollback(&snap_id) {
                    return Err(InstallError::RollbackError(format!(
                        "upgrade failed ({e}), then rollback failed: {rollback_err}"
                    )));
                }
                Err(e)
            }
        }
    }

    /// List all installed packages.
    pub fn list_installed(&self) -> Result<Vec<PackageEntry>, InstallError> {
        let db = PackageDb::load(&self.db_path)?;
        Ok(db.all_packages().cloned().collect())
    }

    /// Search for a package by name substring.
    pub fn search(&self, query: &str) -> Result<Vec<PackageEntry>, InstallError> {
        let db = PackageDb::load(&self.db_path)?;
        let lower = query.to_lowercase();
        Ok(db
            .all_packages()
            .filter(|p| p.name.to_lowercase().contains(&lower))
            .cloned()
            .collect())
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn verify_archive(&self, path: &Path, resolved: &ResolvedPackage) -> Result<(), InstallError> {
        if let PackageSource::Repository { ref checksum, .. } = resolved.source {
            let data = fs::read(path)
                .map_err(|e| InstallError::IoError(format!("cannot read archive: {e}")))?;
            verify_sha256(&data, checksum)?;
        }
        Ok(())
    }

    fn apply_to_filesystem(&self, staging: &Path, _resolved: &ResolvedPackage) -> Result<(), InstallError> {
        copy_dir_contents(staging, &self.target_root)
            .map_err(|e| InstallError::IoError(format!("apply failed: {e}")))
    }

    fn collect_install_paths(&self, resolved: &ResolvedPackage) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Some(ref install) = resolved.manifest.install {
            for file in &install.files {
                paths.push(
                    self.target_root
                        .join(file.path.strip_prefix("/").unwrap_or(&file.path)),
                );
            }
        }
        paths
    }

    fn collect_files_in_dir(&self, dir: &Path) -> Vec<String> {
        let mut files = Vec::new();
        if dir.exists() {
            collect_files_recursive(dir, dir, &mut files);
        }
        files
    }
}

fn copy_dir_contents(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let rel = src_path.strip_prefix(src).unwrap();
        let dst_path = dst.join(rel);
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_dir_contents(&src_path, &dst_path)?;
        } else {
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn collect_files_recursive(base: &Path, dir: &Path, files: &mut Vec<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap().to_string_lossy().to_string();
            let rel_path = format!("/{rel}");
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                files.push(rel_path);
                collect_files_recursive(base, &path, files);
            } else {
                files.push(rel_path);
            }
        }
    }
}

fn extract_archive(archive_path: &Path, output_dir: &Path) -> Result<(), InstallError> {
    fs::create_dir_all(output_dir)
        .map_err(|e| InstallError::IoError(format!("cannot create staging dir: {e}")))?;
    // For now, treat the archive as a flat directory of files.
    // A real implementation would decompress a tar.gz or similar format.
    // We simulate extraction by copying the archive itself as a file.
    let dest_file = output_dir.join("package.tpkg");
    fs::copy(archive_path, &dest_file)
        .map_err(|e| InstallError::IoError(format!("extract failed: {e}")))?;
    Ok(())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::SnapshotManager;
    use sha2::Digest;
    use std::io::Write;

    fn test_pipeline(tmp: &Path) -> InstallPipeline {
        let fetcher = PackageFetcher::new_without_client(tmp.join("packages"));
        let snap_mgr = SnapshotManager::with_root(tmp.join("snapshots"), tmp.to_path_buf());
        let db_path = tmp.join("packages.json");
        let staging = tmp.join("staging");
        InstallPipeline::new(fetcher, snap_mgr, db_path, staging)
    }

    fn make_resolved_package(name: &str, version: &str) -> ResolvedPackage {
        let manifest = TpkgManifest {
            package: tpkg_format::PackageManifest {
                name: tpkg_format::PackageName::new(name).unwrap(),
                version: version.into(),
                description: None,
                license: None,
                authors: vec![],
                dependencies: vec![],
            },
            install: None,
            build: None,
            scripts: None,
        };
        ResolvedPackage {
            name: tpkg_format::PackageName::new(name).unwrap(),
            version: Version::parse(version).unwrap(),
            manifest,
            source: PackageSource::Local {
                path: format!("/tmp/{name}-{version}.tpkg"),
            },
            dependencies: vec![],
        }
    }

    // -----------------------------------------------------------------------
    // Pipeline unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_install_error_display() {
        let err = InstallError::PackageConflict("foo".into());
        assert!(format!("{err}").contains("foo"));

        let err = InstallError::VerificationFailed("checksum mismatch".into());
        assert!(format!("{err}").contains("checksum"));
    }

    #[test]
    fn test_install_with_rollback_on_fetch_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let pipeline = test_pipeline(tmp.path());
        let pkg = make_resolved_package("test-pkg", "1.0.0");

        // This package source points to a non-existent local path
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(pipeline.install(&pkg));
        assert!(result.is_err(), "install should fail because archive doesn't exist");
    }

    #[test]
    fn test_remove_not_installed() {
        let tmp = tempfile::tempdir().unwrap();
        let pipeline = test_pipeline(tmp.path());
        let result = pipeline.remove("nonexistent-pkg");
        assert!(matches!(result, Err(InstallError::PackageNotFound(_))));
    }

    #[test]
    fn test_list_installed_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let pipeline = test_pipeline(tmp.path());
        let list = pipeline.list_installed().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_search_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let pipeline = test_pipeline(tmp.path());
        let results = pipeline.search("anything").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_copy_dir_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");

        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("file1.txt"), b"hello").unwrap();
        fs::write(src.join("sub/file2.txt"), b"world").unwrap();

        copy_dir_contents(&src, &dst).unwrap();
        assert!(dst.join("file1.txt").exists());
        assert!(dst.join("sub/file2.txt").exists());
        assert_eq!(fs::read_to_string(dst.join("file1.txt")).unwrap(), "hello");
    }

    #[test]
    fn test_extract_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.tpkg");
        let output = tmp.path().join("extracted");

        fs::write(&archive, b"fake archive content").unwrap();
        extract_archive(&archive, &output).unwrap();
        assert!(output.join("package.tpkg").exists());
    }

    #[test]
    fn test_verify_archive_valid_checksum() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.tpkg");
        fs::write(&archive, b"some data").unwrap();

        let data = fs::read(&archive).unwrap();
        let mut hasher = sha2::Sha256::new();
        hasher.update(&data);
        let checksum = hex::encode(hasher.finalize());

        let mut pkg = make_resolved_package("test", "1.0.0");
        pkg.source = PackageSource::Repository {
            url: "http://example.com/pkg.tpkg".into(),
            checksum: checksum.clone(),
        };

        let pipeline = test_pipeline(tmp.path());
        let result = pipeline.verify_archive(&archive, &pkg);
        assert!(result.is_ok(), "valid checksum should pass");
    }

    #[test]
    fn test_verify_archive_invalid_checksum() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.tpkg");
        fs::write(&archive, b"some data").unwrap();

        let mut pkg = make_resolved_package("test", "1.0.0");
        pkg.source = PackageSource::Repository {
            url: "http://example.com/pkg.tpkg".into(),
            checksum: "0000000000000000000000000000000000000000000000000000000000000000".into(),
        };

        let pipeline = test_pipeline(tmp.path());
        let result = pipeline.verify_archive(&archive, &pkg);
        assert!(
            matches!(result, Err(InstallError::FetchError(FetchError::ChecksumMismatch { .. }))),
            "invalid checksum should produce ChecksumMismatch, got {:?}",
            result
        );
    }
}
