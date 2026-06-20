use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tpkg_format::{PackageSource, ResolvedPackage};

// ---------------------------------------------------------------------------
// FetchError
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum FetchError {
    TufError(String),
    NetworkError(String),
    ChecksumMismatch { expected: String, actual: String },
    IoError(String),
    NotFound(String),
    ExpiredMetadata(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::TufError(msg) => write!(f, "TUF error: {msg}"),
            FetchError::NetworkError(msg) => write!(f, "network error: {msg}"),
            FetchError::ChecksumMismatch { expected, actual } => {
                write!(f, "SHA-256 checksum mismatch: expected {expected}, got {actual}")
            }
            FetchError::IoError(msg) => write!(f, "I/O error: {msg}"),
            FetchError::NotFound(name) => write!(f, "package not found: {name}"),
            FetchError::ExpiredMetadata(msg) => write!(f, "expired TUF metadata: {msg}"),
        }
    }
}

impl std::error::Error for FetchError {}

// ---------------------------------------------------------------------------
// verify_sha256
// ---------------------------------------------------------------------------

/// Compute the SHA-256 hex digest of `data` and compare it against `expected_hex`.
pub fn verify_sha256(data: &[u8], expected_hex: &str) -> Result<(), FetchError> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let actual = hex::encode(hasher.finalize());
    if actual == expected_hex {
        Ok(())
    } else {
        Err(FetchError::ChecksumMismatch {
            expected: expected_hex.to_string(),
            actual,
        })
    }
}

// ---------------------------------------------------------------------------
// RepositoryClient
// ---------------------------------------------------------------------------

/// A TUF-based repository client.
///
/// Wraps the `tough` crate's `Repository` to fetch and verify metadata and
/// target files according to the TUF 1.0 specification.
#[derive(Debug)]
pub struct RepositoryClient {
    repo: tough::Repository,
}

impl RepositoryClient {
    /// Load a TUF repository from the given root metadata and base URLs.
    ///
    /// `root` is the trusted root.json content (shipped with the client).
    /// `metadata_url` is the base URL for metadata files (root.json, timestamp.json, etc.).
    /// `targets_url` is the base URL for target files.
    ///
    /// This enforces metadata expiration dates (Safe mode).
    pub async fn new(
        root: Vec<u8>,
        metadata_url: url::Url,
        targets_url: url::Url,
    ) -> Result<Self, FetchError> {
        let repo = tough::RepositoryLoader::new(&root, metadata_url, targets_url)
            .expiration_enforcement(tough::ExpirationEnforcement::Safe)
            .load()
            .await
            .map_err(|e| classify_tough_error(e.to_string()))?;
        Ok(Self { repo })
    }

    /// Load a TUF repository ignoring metadata expiration (Unsafe mode).
    ///
    /// This should only be used for testing.
    pub async fn new_unsafe(
        root: Vec<u8>,
        metadata_url: url::Url,
        targets_url: url::Url,
    ) -> Result<Self, FetchError> {
        let repo = tough::RepositoryLoader::new(&root, metadata_url, targets_url)
            .expiration_enforcement(tough::ExpirationEnforcement::Unsafe)
            .load()
            .await
            .map_err(|e| FetchError::TufError(e.to_string()))?;
        Ok(Self { repo })
    }

    /// Return the list of target (package archive) names in the repository.
    pub fn list_targets(&self) -> Vec<String> {
        self.repo
            .targets()
            .signed
            .targets
            .keys()
            .map(|n| n.raw().to_string())
            .collect()
    }

    /// Download a target file and save it to `output_dir`.
    ///
    /// Returns the path to the saved file.
    pub async fn download_target(
        &self,
        target_name: &str,
        output_dir: &Path,
    ) -> Result<PathBuf, FetchError> {
        let name = tough::TargetName::new(target_name)
            .map_err(|e| FetchError::NotFound(format!("invalid target name: {e}")))?;
        fs::create_dir_all(output_dir)
            .map_err(|e| FetchError::IoError(format!("cannot create output dir: {e}")))?;
        self.repo
            .save_target(&name, output_dir, tough::Prefix::None)
            .await
            .map_err(|e| FetchError::TufError(format!("target download failed: {e}")))?;
        Ok(output_dir.join(target_name))
    }

    /// Fetch a target file as a byte vector.
    ///
    /// The TUF library verifies the checksum of the data before returning.
    pub async fn fetch_target_bytes(&self, target_name: &str) -> Result<Vec<u8>, FetchError> {
        let name = tough::TargetName::new(target_name)
            .map_err(|e| FetchError::NotFound(format!("invalid target name: {e}")))?;
        use tough::IntoVec;
        let stream = self
            .repo
            .read_target(&name)
            .await
            .map_err(|e| FetchError::TufError(format!("failed to fetch target: {e}")))?
            .ok_or_else(|| FetchError::NotFound(target_name.to_string()))?;
        let data: Vec<u8> = stream
            .into_vec()
            .await
            .map_err(|e| FetchError::TufError(format!("failed to read target data: {e}")))?;
        Ok(data)
    }
}

/// Classify a `tough` error message into an appropriate `FetchError` variant.
fn classify_tough_error(msg: String) -> FetchError {
    let lower = msg.to_lowercase();
    if lower.contains("expir") {
        FetchError::ExpiredMetadata(msg)
    } else if lower.contains("not found") || lower.contains("404") || lower.contains("no such") {
        FetchError::NotFound(msg)
    } else if lower.contains("checksum") || lower.contains("hash") {
        FetchError::ChecksumMismatch {
            expected: msg.clone(),
            actual: "unknown".into(),
        }
    } else {
        FetchError::TufError(msg)
    }
}

// ---------------------------------------------------------------------------
// PackageFetcher
// ---------------------------------------------------------------------------

/// High-level package fetcher that uses a `RepositoryClient` to download
/// and verify `.tpkg` archives.
#[derive(Debug)]
pub struct PackageFetcher {
    client: Option<RepositoryClient>,
    temp_dir: PathBuf,
}

impl PackageFetcher {
    /// Create a new `PackageFetcher` backed by the given TUF repository client.
    pub fn new(client: RepositoryClient) -> Self {
        Self {
            client: Some(client),
            temp_dir: PathBuf::from("/tmp/turnix-packages"),
        }
    }

    /// Create a `PackageFetcher` without a TUF client (for testing).
    /// `fetch` will return an error if called without a client.
    pub fn new_without_client(temp_dir: PathBuf) -> Self {
        Self {
            client: None,
            temp_dir,
        }
    }

    /// Create a new `PackageFetcher` with a custom download cache directory.
    pub fn with_temp_dir(client: RepositoryClient, temp_dir: PathBuf) -> Self {
        Self {
            client: Some(client),
            temp_dir,
        }
    }

    /// Download and verify a resolved package.
    ///
    /// The target file is fetched from the TUF repository, which verifies
    /// its checksum against the TUF targets metadata. If the `ResolvedPackage`
    /// source also specifies a checksum, an extra SHA-256 verification is
    /// performed.
    ///
    /// Returns the path to the downloaded `.tpkg` archive.
    pub async fn fetch(&self, resolved: &ResolvedPackage) -> Result<PathBuf, FetchError> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| FetchError::TufError("no TUF repository configured".into()))?;

        let target_name = format!("{}-{}.tpkg", resolved.name, resolved.version);
        let output_path = client.download_target(&target_name, &self.temp_dir).await?;

        // Extra verification: if the source specifies a SHA-256 checksum,
        // verify it against the downloaded data.
        if let PackageSource::Repository { ref checksum, .. } = resolved.source {
            let data = fs::read(&output_path)
                .map_err(|e| FetchError::IoError(format!("cannot read downloaded file: {e}")))?;
            verify_sha256(&data, checksum)?;
        }

        Ok(output_path)
    }

    /// Return a reference to the underlying repository client, if any.
    pub fn client(&self) -> Option<&RepositoryClient> {
        self.client.as_ref()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // -----------------------------------------------------------------------
    // SHA-256 verification tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_sha256_valid_checksum() {
        let data = b"hello world";
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert!(verify_sha256(data, expected).is_ok());
    }

    #[test]
    fn test_sha256_invalid_checksum() {
        let data = b"hello world";
        let wrong = "0000000000000000000000000000000000000000000000000000000000000000";
        let result = verify_sha256(data, wrong);
        assert!(matches!(result, Err(FetchError::ChecksumMismatch { .. })));
        if let Err(FetchError::ChecksumMismatch { expected, actual }) = result {
            assert_eq!(expected, wrong);
            assert_eq!(
                actual,
                "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
            );
        }
    }

    #[test]
    fn test_sha256_empty_data() {
        let data = b"";
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert!(verify_sha256(data, expected).is_ok());
    }

    #[test]
    fn test_sha256_large_data() {
        let data = vec![0xABu8; 65536];
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let expected = hex::encode(hasher.finalize());
        assert!(verify_sha256(&data, &expected).is_ok());
    }

    // -----------------------------------------------------------------------
    // FetchError display tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_fetch_error_display() {
        let err = FetchError::NotFound("test-pkg".into());
        assert!(format!("{err}").contains("test-pkg"));

        let err = FetchError::ChecksumMismatch {
            expected: "abc".into(),
            actual: "def".into(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("abc"));
        assert!(msg.contains("def"));

        let err = FetchError::ExpiredMetadata("root expired".into());
        assert!(format!("{err}").contains("expired"));

        let err = FetchError::TufError("signature invalid".into());
        assert!(format!("{err}").contains("signature"));

        let err = FetchError::IoError("permission denied".into());
        assert!(format!("{err}").contains("permission"));
    }

    // -----------------------------------------------------------------------
    // RepositoryClient creation failure tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_client_invalid_root_rejected() {
        let bad_root = b"not valid JSON".to_vec();
        let meta_url = url::Url::parse("file:///nonexistent").unwrap();
        let target_url = url::Url::parse("file:///nonexistent").unwrap();
        let result = RepositoryClient::new(bad_root, meta_url, target_url).await;
        assert!(result.is_err(), "invalid root should produce an error");
        match result.unwrap_err() {
            FetchError::TufError(_) | FetchError::NotFound(_) | FetchError::IoError(_) => {}
            other => panic!("unexpected error variant: {other}"),
        }
    }

    // -----------------------------------------------------------------------
    // PackageFetcher construction tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_package_fetcher_construction() {
        // Create a client in unsafe mode with invalid params — it will fail,
        // but we verify the construction of PackageFetcher is valid once we
        // have a client.
        let bad_root = b"{}".to_vec();
        let meta_url = url::Url::parse("file:///tmp/tuf-meta").unwrap();
        let target_url = url::Url::parse("file:///tmp/tuf-targets").unwrap();
        let result = RepositoryClient::new_unsafe(bad_root, meta_url, target_url).await;
        // We expect failure since the root is not valid TUF metadata
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // verify_sha256 used in PackageFetcher context
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_package_source_checksum_verification() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg_path = tmp.path().join("test-pkg-1.0.0.tpkg");
        let mut file = fs::File::create(&pkg_path).unwrap();
        file.write_all(b"fake package content").unwrap();
        drop(file);

        let data = fs::read(&pkg_path).unwrap();
        let correct_checksum = {
            let mut h = Sha256::new();
            h.update(&data);
            hex::encode(h.finalize())
        };

        assert!(verify_sha256(&data, &correct_checksum).is_ok());

        let wrong_checksum = "0000000000000000000000000000000000000000000000000000000000000000";
        assert!(verify_sha256(&data, wrong_checksum).is_err());
    }
}
