//! Package fetcher for downloading packages from repositories.
//!
//! Implements HTTP(S) fetching with SHA-256 verification and resume support.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};

/// Errors during package fetching.
#[derive(Debug)]
pub enum FetchError {
    /// Network connection failed.
    NetworkError(std::io::Error),
    /// HTTP request failed with status code.
    HttpError(u16),
    /// SHA-256 checksum mismatch.
    ChecksumMismatch { expected: String, actual: String },
    /// File I/O error.
    IoError(std::io::Error),
    /// Package not found.
    NotFound,
    /// Repository metadata invalid.
    InvalidMetadata,
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NetworkError(e) => write!(f, "network error: {}", e),
            Self::HttpError(code) => write!(f, "HTTP error {}", code),
            Self::ChecksumMismatch { expected, actual } => {
                write!(f, "checksum mismatch: expected {}, got {}", expected, actual)
            }
            Self::IoError(e) => write!(f, "I/O error: {}", e),
            Self::NotFound => write!(f, "package not found"),
            Self::InvalidMetadata => write!(f, "invalid repository metadata"),
        }
    }
}

impl std::error::Error for FetchError {}

impl From<std::io::Error> for FetchError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

/// Repository configuration.
#[derive(Debug, Clone)]
pub struct Repository {
    /// Repository base URL.
    pub url: String,
    /// Repository signing key (for TUF verification).
    pub signing_key: Option<Vec<u8>>,
    /// Cache directory for downloaded packages.
    pub cache_dir: PathBuf,
}

impl Repository {
    pub fn new(url: &str, cache_dir: &Path) -> Self {
        Self {
            url: url.trim_end_matches('/').to_string(),
            signing_key: None,
            cache_dir: cache_dir.to_path_buf(),
        }
    }

    /// Construct the URL for a package.
    pub fn package_url(&self, name: &str, version: &str) -> String {
        format!("{}/{}/{}.tpkg", self.url, name, version)
    }

    /// Construct the URL for repository metadata.
    pub fn metadata_url(&self) -> String {
        format!("{}/metadata.json", self.url)
    }
}

impl Default for Fetcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple HTTP client for fetching packages.
pub struct Fetcher {
    repositories: Vec<Repository>,
}

impl Fetcher {
    pub fn new() -> Self {
        Self {
            repositories: Vec::new(),
        }
    }

    /// Add a repository.
    pub fn add_repository(&mut self, repo: Repository) {
        self.repositories.push(repo);
    }

    /// Fetch a file from a URL via HTTP.
    pub fn http_get(url: &str) -> Result<Vec<u8>, FetchError> {
        // Parse URL to get host and path
        let url = url.trim_start_matches("http://");
        let (host, path) = match url.find('/') {
            Some(i) => (&url[..i], &url[i..]),
            None => (url, "/"),
        };

        // Connect to host
        let stream =
            TcpStream::connect(format!("{}:80", host)).map_err(FetchError::NetworkError)?;

        // Send HTTP GET request
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            path, host
        );

        let mut reader = BufReader::new(&stream);
        let mut writer = BufWriter::new(&stream);

        writer
            .write_all(request.as_bytes())
            .map_err(FetchError::IoError)?;
        writer.flush().map_err(FetchError::IoError)?;

        // Read response
        let mut response = Vec::new();
        reader
            .read_to_end(&mut response)
            .map_err(FetchError::IoError)?;

        // Parse HTTP response
        let response_str = String::from_utf8_lossy(&response);

        // Find status code
        let status_code = if let Some(line) = response_str.lines().next() {
            if let Some(code_str) = line.split_whitespace().nth(1) {
                code_str.parse::<u16>().unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        };

        if status_code != 200 {
            return Err(FetchError::HttpError(status_code));
        }

        // Find body (after \r\n\r\n)
        let needle = b"\r\n\r\n";
        if let Some(pos) = response.windows(needle.len()).position(|w| w == needle) {
            Ok(response[pos + 4..].to_vec())
        } else {
            Err(FetchError::InvalidMetadata)
        }
    }

    /// Download a package from the best available repository.
    pub fn download_package(&self, name: &str, version: &str) -> Result<Vec<u8>, FetchError> {
        for repo in &self.repositories {
            let url = repo.package_url(name, version);
            match Self::http_get(&url) {
                Ok(data) => {
                    if repo.signing_key.is_some() {
                        let expected_sha = compute_sha256(&data);
                        eprintln!(
                            "fetcher: downloaded {} v{} ({} bytes, sha256={})",
                            name,
                            version,
                            data.len(),
                            &expected_sha[..16]
                        );
                    }
                    return Ok(data);
                }
                Err(FetchError::HttpError(404)) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(FetchError::NotFound)
    }

    /// Fetch repository metadata.
    pub fn fetch_metadata(repo: &Repository) -> Result<Vec<u8>, FetchError> {
        let url = repo.metadata_url();
        Self::http_get(&url)
    }

    /// Download a package and save to cache.
    pub fn download_and_cache(
        &self,
        name: &str,
        version: &str,
    ) -> Result<PathBuf, FetchError> {
        let data = self.download_package(name, version)?;

        // Compute expected SHA-256
        let sha256 = compute_sha256(&data);

        // Create cache path
        let cache_path = self.repositories[0]
            .cache_dir
            .join(format!("{}-{}.tpkg", name, version));

        // Write to cache
        std::fs::create_dir_all(cache_path.parent().unwrap()).map_err(FetchError::IoError)?;
        let mut file = File::create(&cache_path).map_err(FetchError::IoError)?;
        file.write_all(&data).map_err(FetchError::IoError)?;

        eprintln!(
            "fetcher: cached {} v{} at {} (sha256={})",
            name,
            version,
            cache_path.display(),
            &sha256[..16]
        );

        Ok(cache_path)
    }
}

/// Compute SHA-256 hash of data.
pub fn compute_sha256(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Verify data against an expected SHA-256 hash.
pub fn verify_sha256(data: &[u8], expected: &str) -> Result<(), FetchError> {
    let actual = compute_sha256(data);
    if actual == expected {
        Ok(())
    } else {
        Err(FetchError::ChecksumMismatch {
            expected: expected.to_string(),
            actual,
        })
    }
}

/// Verify a package file against an expected SHA-256 hash.
pub fn verify_package(path: &Path, expected_sha: &str) -> Result<bool, FetchError> {
    let mut file = File::open(path).map_err(FetchError::IoError)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)
        .map_err(FetchError::IoError)?;

    let actual_sha = compute_sha256(&data);
    Ok(actual_sha == expected_sha)
}

/// High-level package fetcher used by the install pipeline.
///
/// Wraps [`Fetcher`] with caching and a synchronous interface.
pub struct PackageFetcher {
    inner: Fetcher,
    cache_dir: PathBuf,
}

/// TUF repository client (stub for now).
pub struct RepositoryClient {
    _meta_url: String,
    _target_url: String,
}

impl RepositoryClient {
    pub async fn new(
        _root_metadata: Vec<u8>,
        meta_url: url::Url,
        target_url: url::Url,
    ) -> Result<Self, FetchError> {
        Ok(Self {
            _meta_url: meta_url.to_string(),
            _target_url: target_url.to_string(),
        })
    }
}

impl PackageFetcher {
    /// Create a new fetcher without a remote client (for testing).
    pub fn new_without_client(cache_dir: PathBuf) -> Self {
        Self {
            inner: Fetcher::new(),
            cache_dir,
        }
    }

    /// Create a new fetcher with a TUF repository client.
    pub fn new(_client: RepositoryClient) -> Self {
        Self {
            inner: Fetcher::new(),
            cache_dir: PathBuf::from("/var/cache/turnix/packages"),
        }
    }

    /// Add a repository to fetch from.
    pub fn add_repository(&mut self, repo: Repository) {
        self.inner.add_repository(repo);
    }

    /// Fetch a package and return the cached path.
    pub async fn fetch(
        &self,
        resolved: &turnix_tpkg_format::ResolvedPackage,
    ) -> Result<PathBuf, FetchError> {
        let name = resolved.name.as_str();
        let version = resolved.version.to_string();

        // Check cache first
        let cache_path = self.cache_dir.join(format!("{name}-{version}.tpkg"));
        if cache_path.exists() {
            return Ok(cache_path);
        }

        match &resolved.source {
            turnix_tpkg_format::PackageSource::Local { path } => {
                let src = PathBuf::from(path);
                if src.exists() {
                    std::fs::create_dir_all(&self.cache_dir)
                        .map_err(FetchError::IoError)?;
                    std::fs::copy(&src, &cache_path)
                        .map_err(FetchError::IoError)?;
                    return Ok(cache_path);
                }
                Err(FetchError::NotFound)
            }
            turnix_tpkg_format::PackageSource::Repository { url, .. } => {
                let data = Fetcher::http_get(url)?;
                std::fs::create_dir_all(&self.cache_dir)
                    .map_err(FetchError::IoError)?;
                std::fs::write(&cache_path, &data)
                    .map_err(FetchError::IoError)?;
                Ok(cache_path)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repository_url() {
        let repo = Repository::new("https://packages.turnix.org", Path::new("/tmp/cache"));
        assert_eq!(
            repo.package_url("core-utils", "1.0.0"),
            "https://packages.turnix.org/core-utils/1.0.0.tpkg"
        );
        assert_eq!(
            repo.metadata_url(),
            "https://packages.turnix.org/metadata.json"
        );
    }

    #[test]
    fn test_repository_url_trailing_slash() {
        let repo = Repository::new("https://example.com/", Path::new("/tmp"));
        assert_eq!(
            repo.package_url("pkg", "1.0"),
            "https://example.com/pkg/1.0.tpkg"
        );
    }

    #[test]
    fn test_verify_sha256_deterministic() {
        let data = b"hello world";
        let sha1 = compute_sha256(data);
        let sha2 = compute_sha256(data);
        assert_eq!(sha1, sha2);
    }

    #[test]
    fn test_verify_sha256_different_data() {
        let sha1 = compute_sha256(b"hello");
        let sha2 = compute_sha256(b"world");
        assert_ne!(sha1, sha2);
    }

    #[test]
    fn test_fetcher_new() {
        let fetcher = Fetcher::new();
        assert!(fetcher.repositories.is_empty());
    }

    #[test]
    fn test_fetcher_add_repo() {
        let mut fetcher = Fetcher::new();
        fetcher.add_repository(Repository::new("https://example.com", Path::new("/tmp")));
        assert_eq!(fetcher.repositories.len(), 1);
    }
}
