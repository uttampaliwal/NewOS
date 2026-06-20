use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::Sha256;
use hmac::{Hmac, Mac};

// ---------------------------------------------------------------------------
// Log level
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

// ---------------------------------------------------------------------------
// Log entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: u64,
    pub level: LogLevel,
    pub source: String,
    pub message: String,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    /// Hex-encoded HMAC-SHA256 over all other fields (None means unsealed).
    #[serde(default)]
    pub hmac: Option<String>,
}

impl LogEntry {
    /// Create a new log entry with the current timestamp.
    pub fn new(level: LogLevel, source: &str, message: &str) -> Self {
        Self {
            timestamp: now_secs(),
            level,
            source: source.to_string(),
            message: message.to_string(),
            fields: BTreeMap::new(),
            hmac: None,
        }
    }

    pub fn with_field(mut self, key: &str, value: &str) -> Self {
        self.fields.insert(key.to_string(), value.to_string());
        self
    }

    /// Serialise to JSON bytes.
    pub fn to_json(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(self).map_err(|e| format!("json serialisation failed: {e}"))
    }

    /// Deserialise from JSON bytes.
    pub fn from_json(data: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(data).map_err(|e| format!("json deserialisation failed: {e}"))
    }

    /// Return the canonical bytes used for HMAC computation (all fields except hmac).
    fn canonical_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.timestamp.to_le_bytes());
        buf.extend_from_slice(self.level.as_str().as_bytes());
        buf.extend_from_slice(self.source.as_bytes());
        buf.extend_from_slice(self.message.as_bytes());
        for (k, v) in &self.fields {
            buf.extend_from_slice(k.as_bytes());
            buf.extend_from_slice(v.as_bytes());
        }
        buf
    }

    /// Seal this entry with an HMAC-SHA256 using the given key.
    pub fn seal(&mut self, key: &[u8]) {
        let mut mac = Hmac::<Sha256>::new_from_slice(key)
            .expect("HMAC key should be valid");
        mac.update(&self.canonical_bytes());
        let result = mac.finalize();
        self.hmac = Some(hex::encode(result.into_bytes()));
    }

    /// Verify the HMAC on this entry. Returns true if the HMAC matches.
    pub fn verify(&self, key: &[u8]) -> bool {
        let hmac_str = match &self.hmac {
            Some(h) => h,
            None => return false, // no HMAC to verify
        };
        let expected_bytes = match hex::decode(hmac_str) {
            Ok(b) => b,
            Err(_) => return false,
        };

        let mut mac = Hmac::<Sha256>::new_from_slice(key)
            .expect("HMAC key should be valid");
        mac.update(&self.canonical_bytes());
        mac.verify_slice(&expected_bytes).is_ok()
    }
}

// ---------------------------------------------------------------------------
// Log rotation
// ---------------------------------------------------------------------------

pub const ROTATION_SIZE: u64 = 10 * 1024 * 1024; // 10 MB
pub const ROTATION_AGE: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours
pub const MAX_ROTATED_FILES: usize = 7;

/// Manages log file rotation based on size and age.
pub struct LogRotator {
    base_path: PathBuf,
    current_size: u64,
    created_at: SystemTime,
    file: File,
}

impl LogRotator {
    /// Open or create the log file at `base_path.log`. If a file already
    /// exists, its size is used as the starting size.
    pub fn open(base_path: &Path) -> Result<Self, String> {
        let log_path = base_path.with_extension("log");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| format!("cannot open log file {:?}: {e}", log_path))?;

        let current_size = fs::metadata(&log_path)
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(Self {
            base_path: base_path.to_path_buf(),
            current_size,
            created_at: SystemTime::now(),
            file,
        })
    }

    /// Write a byte slice to the log file. Rotates first if thresholds are
    /// exceeded, then appends.
    pub fn write(&mut self, data: &[u8]) -> Result<(), String> {
        if self.should_rotate() {
            self.rotate()?;
        }
        self.file
            .write_all(data)
            .map_err(|e| format!("write failed: {e}"))?;
        self.current_size += data.len() as u64;
        Ok(())
    }

    /// Check whether rotation thresholds have been exceeded.
    pub fn should_rotate(&self) -> bool {
        if self.current_size >= ROTATION_SIZE {
            return true;
        }
        if self.created_at.elapsed().map_or(false, |elapsed| elapsed >= ROTATION_AGE) {
            return true;
        }
        false
    }

    /// Execute log rotation: rename existing files and create a new log file.
    pub fn rotate(&mut self) -> Result<(), String> {
        // Close current file
        // (flush is automatic on drop, but we explicitly sync)
        self.file
            .sync_all()
            .map_err(|e| format!("sync failed: {e}"))?;

        let log_path = self.base_path.with_extension("log");

        // Shift rotated files: log.6 → log.7, log.5 → log.6, ..., log.1 → log.2
        for i in (1..MAX_ROTATED_FILES).rev() {
            let src = self.rotated_path(i);
            let dst = self.rotated_path(i + 1);
            if src.exists() {
                let _ = fs::rename(&src, &dst);
            }
        }

        // Rename current log → log.1
        let rotated = self.rotated_path(1);
        let _ = fs::rename(&log_path, &rotated);

        // Open new log file
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| format!("cannot reopen log {:?}: {e}", log_path))?;

        self.file = file;
        self.current_size = 0;
        self.created_at = SystemTime::now();

        Ok(())
    }

    /// Get the path for rotated file index i (e.g. log.1, log.2, ...).
    fn rotated_path(&self, i: usize) -> PathBuf {
        let p = self.base_path.with_extension("log");
        let name = format!("{}.{}", p.to_string_lossy(), i);
        PathBuf::from(name)
    }
}

// ---------------------------------------------------------------------------
// Kernel log forwarder (stub)
// ---------------------------------------------------------------------------

/// Represents a single log line from the kernel serial ring buffer.
#[derive(Debug, Clone)]
pub struct KernelLogLine {
    pub level: LogLevel,
    pub message: String,
}

/// Interface for reading kernel log entries.
/// In production this reads from the kernel ring buffer via a syscall or
/// shared memory region.  For the task spec the struct is testable via
/// injection.
pub trait KernelLogSource {
    /// Poll for new kernel log lines.  Returns `None` when no more lines
    /// are available.
    fn poll(&mut self) -> Option<KernelLogLine>;
}

/// Trivial no-op implementation for testing.
pub struct NullKernelLogSource;

impl KernelLogSource for NullKernelLogSource {
    fn poll(&mut self) -> Option<KernelLogLine> {
        None
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── 44.2: HMAC sealing ─────────────────────────────────────────────────

    #[test]
    fn test_hmac_seal_and_verify() {
        let key = b"turnix-log-secret-key-0000";
        let mut entry = LogEntry::new(LogLevel::Info, "test", "hello world");
        entry.seal(key);
        assert!(entry.hmac.is_some());
        assert!(entry.verify(key));
    }

    #[test]
    fn test_tampered_entry_fails_verification() {
        let key = b"turnix-log-secret-key-0000";
        let mut entry = LogEntry::new(LogLevel::Warn, "auth", "login failed");
        entry.seal(key);
        assert!(entry.verify(key));

        // Tamper the message
        entry.message = "login succeeded".to_string();
        assert!(!entry.verify(key));
    }

    #[test]
    fn test_tampered_fields_fails_verification() {
        let key = b"secret-key";
        let mut entry = LogEntry::new(LogLevel::Error, "db", "timeout")
            .with_field("user", "alice");
        entry.seal(key);
        assert!(entry.verify(key));

        // Tamper a field
        entry.fields.insert("user".into(), "mallory".into());
        assert!(!entry.verify(key));
    }

    #[test]
    fn test_no_hmac_fails_verification() {
        let entry = LogEntry::new(LogLevel::Debug, "test", "no hmac");
        assert!(!entry.verify(b"key"));
    }

    #[test]
    fn test_unsealed_then_sealed() {
        let key = b"key";
        let mut entry = LogEntry::new(LogLevel::Info, "svc", "started");
        assert!(entry.hmac.is_none());
        entry.seal(key);
        assert!(entry.hmac.is_some());
        assert!(entry.verify(key));
    }

    #[test]
    fn test_different_keys_fail_verification() {
        let mut entry = LogEntry::new(LogLevel::Info, "test", "secret");
        entry.seal(b"key-a");
        assert!(entry.verify(b"key-a"));
        assert!(!entry.verify(b"key-b"));
    }

    #[test]
    fn test_hmac_different_for_different_content() {
        let key = b"key";
        let mut e1 = LogEntry::new(LogLevel::Info, "src", "msg");
        e1.seal(key);
        let mut e2 = LogEntry::new(LogLevel::Error, "src", "msg");
        e2.seal(key);
        assert_ne!(e1.hmac, e2.hmac);
    }

    #[test]
    fn test_json_round_trip_with_hmac() {
        let key = b"json-test-key";
        let mut entry = LogEntry::new(LogLevel::Info, "test", "json round-trip")
            .with_field("pid", "42");
        entry.seal(key);

        let json = entry.to_json().unwrap();
        let decoded = LogEntry::from_json(&json).unwrap();
        assert_eq!(entry.timestamp, decoded.timestamp);
        assert_eq!(entry.level, decoded.level);
        assert_eq!(entry.message, decoded.message);
        assert_eq!(entry.hmac, decoded.hmac);
        assert!(decoded.verify(key));
    }

    // ── 44.2: Log rotation ─────────────────────────────────────────────────

    #[test]
    fn test_rotation_size_threshold() {
        // The threshold is 10MB, so a "small" rotator shouldn't rotate,
        // but we can test the should_rotate logic by checking constants.
        assert_eq!(ROTATION_SIZE, 10 * 1024 * 1024);
        assert_eq!(ROTATION_AGE, Duration::from_secs(86400));
        assert_eq!(MAX_ROTATED_FILES, 7);
    }

    #[test]
    fn test_rotation_creates_rotated_files() {
        let dir = std::env::temp_dir().join(format!("logd_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let base = dir.join("test-app");

        let mut rotator = LogRotator::open(&base).unwrap();

        // Write enough data to trigger rotation (just over 10MB)
        // We cheat by setting current_size directly (via many writes)
        let chunk = vec![b'X'; 8192];
        // 10MB / 8KB ≈ 1280 writes → but we only need to trigger rotation once
        for _ in 0..5 {
            rotator.write(&chunk).unwrap();
        }
        assert!(!rotator.should_rotate()); // still small

        // Manually advance size past threshold
        rotator.current_size = ROTATION_SIZE + 1;
        assert!(rotator.should_rotate());

        rotator.rotate().unwrap();

        // After rotation, the .log.1 file should exist
        let rotated = dir.join("test-app.log.1");
        assert!(rotated.exists(), "rotated file should exist");

        // New file should be empty
        assert_eq!(rotator.current_size, 0);

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_rotation_retention_limit() {
        // Verify that rotating MAX_ROTATED_FILES+1 times only keeps
        // MAX_ROTATED_FILES files.
        let dir = std::env::temp_dir().join(format!("logd_ret_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let base = dir.join("rotatee");

        let mut rotator = LogRotator::open(&base).unwrap();

        for i in 0..=MAX_ROTATED_FILES + 1 {
            rotator.current_size = ROTATION_SIZE + 1; // force rotation
            rotator.rotate().unwrap();
            // Write a marker so we can identify this generation
            rotator.write(format!("generation {i}\n").as_bytes()).unwrap();
        }

        // Count rotated files
        let mut count = 0;
        for i in 1..=MAX_ROTATED_FILES + 2 {
            let p = dir.join(format!("rotatee.log.{i}"));
            if p.exists() {
                count += 1;
            }
        }
        assert!(count <= MAX_ROTATED_FILES, "at most {MAX_ROTATED_FILES} rotated files, got {count}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_kernel_log_source_stub() {
        let mut source = NullKernelLogSource;
        assert!(source.poll().is_none());
    }

    #[test]
    fn test_log_level_as_str() {
        assert_eq!(LogLevel::Debug.as_str(), "DEBUG");
        assert_eq!(LogLevel::Info.as_str(), "INFO");
        assert_eq!(LogLevel::Warn.as_str(), "WARN");
        assert_eq!(LogLevel::Error.as_str(), "ERROR");
    }
}
