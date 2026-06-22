//! Integrity Measurement Architecture (IMA), EVM, and stack canaries.
//!
//! * IMA: SHA-256 measurement of each executed binary, stored in a kernel
//!   measurement log (ring buffer).
//! * EVM: HMAC-SHA256 verification of file metadata extended attributes
//!   (stub — wired once VFS xattr support is available).
//! * Stack canary: a random 64‑bit value placed at the bottom of each kernel
//!   stack, checked on every context switch.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ---------------------------------------------------------------------------
// SHA-256 implementation (FIPS 180-4)
// ---------------------------------------------------------------------------

struct Sha256 {
    state: [u32; 8],
    count: u64,
    buffer: [u8; 64],
}

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            count: 0,
            buffer: [0u8; 64],
        }
    }

    fn update(&mut self, data: &[u8]) {
        for &byte in data {
            let idx = (self.count % 64) as usize;
            self.buffer[idx] = byte;
            self.count += 1;
            if idx == 63 {
                self.compress();
            }
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        let original_count = self.count;
        self.update(&[0x80]);
        while self.count % 64 != 56 {
            self.update(&[0x00]);
        }
        let bits = original_count * 8;
        for i in (0..8).rev() {
            self.update(&[(bits >> (i * 8)) as u8]);
        }
        let mut hash = [0u8; 32];
        for (i, &word) in self.state.iter().enumerate() {
            hash[i * 4] = (word >> 24) as u8;
            hash[i * 4 + 1] = (word >> 16) as u8;
            hash[i * 4 + 2] = (word >> 8) as u8;
            hash[i * 4 + 3] = word as u8;
        }
        hash
    }

    fn compress(&mut self) {
        let k: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
            0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
            0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
            0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
            0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
            0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
            0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
            0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
            0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
            0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
            0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
            0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
            0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
            0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
        ];

        let mut w = [0u32; 64];
        for (i, slot) in w.iter_mut().enumerate().take(16) {
            *slot = (self.buffer[i * 4] as u32) << 24
                | (self.buffer[i * 4 + 1] as u32) << 16
                | (self.buffer[i * 4 + 2] as u32) << 8
                | (self.buffer[i * 4 + 3] as u32);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(k[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

/// Compute SHA-256 hash of data.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize()
}

// ---------------------------------------------------------------------------
// HMAC-SHA256 (for EVM)
// ---------------------------------------------------------------------------

/// Compute HMAC-SHA256(key, data).
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;
    let mut k = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let hash = sha256(key);
        k[..32].copy_from_slice(&hash);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    for byte in k.iter_mut() {
        *byte ^= 0x36;
    }
    let inner = sha256(&[&k[..], data].concat());
    for byte in k.iter_mut() {
        *byte ^= 0x36 ^ 0x5c;
    }
    sha256(&[&k[..], &inner].concat())
}

// ---------------------------------------------------------------------------
// IMA measurement log
// ---------------------------------------------------------------------------

/// A single IMA measurement entry.
#[derive(Debug, Clone)]
pub struct ImaMeasurement {
    /// PCR index (typically 10 for IMA).
    pub pcr: u32,
    /// SHA-256 hash of the measured binary.
    pub hash: [u8; 32],
    /// Path of the measured file.
    pub path: Vec<u8>,
}

/// Maximum number of measurements to keep (ring buffer).
const IMA_LOG_MAX: usize = 4096;

static IMA_LOG: Mutex<Vec<ImaMeasurement>> = Mutex::new(Vec::new());

/// Record a new IMA measurement for an executed binary.
pub fn measure_exec(elf_data: &[u8], path: &str) {
    let hash = sha256(elf_data);
    let mut log = IMA_LOG.lock();
    if log.len() >= IMA_LOG_MAX {
        log.remove(0); // Ring-buffer eviction
    }
    log.push(ImaMeasurement {
        pcr: 10,
        hash,
        path: path.as_bytes().to_vec(),
    });
}

/// Return a copy of the current measurement log.
pub fn get_measurement_log() -> Vec<ImaMeasurement> {
    IMA_LOG.lock().clone()
}

// ---------------------------------------------------------------------------
// EVM — Extended Verification Module (stub)
// ---------------------------------------------------------------------------

/// Verify an EVM HMAC for the given file metadata.
///
/// Recomputes HMAC-SHA256 over (inode, size, mtime) using the system key
/// and compares against the stored HMAC.
pub fn evm_verify(
    inode: u64,
    size: u64,
    mtime: u64,
    stored_hmac: &[u8; 32],
) -> bool {
    let computed = evm_compute_hmac(inode, size, mtime);
    // Constant-time comparison to prevent timing attacks
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= computed[i] ^ stored_hmac[i];
    }
    diff == 0
}

/// System EVM HMAC key (32 bytes). In production this would be derived from
/// a TPM-stored secret; here we use a fixed key for boot-time integrity.
static EVM_HMAC_KEY: Mutex<Option<[u8; 32]>> = Mutex::new(None);

/// Fallback key used when TPM is not available.
const EVM_HMAC_KEY_FALLBACK: &[u8; 32] = b"turnix-evm-hmac-key-2024-v1!1234";

/// Set the EVM HMAC key (called during boot from TPM-derived random bytes).
pub fn set_evm_key(key: [u8; 32]) {
    *EVM_HMAC_KEY.lock() = Some(key);
}

/// Get the EVM HMAC key, initializing from TPM if available.
fn get_evm_key() -> [u8; 32] {
    let key_guard = EVM_HMAC_KEY.lock();
    if let Some(key) = *key_guard {
        return key;
    }
    drop(key_guard);

    // Try to derive from TPM (only on real hardware, not in test builds)
    #[cfg(not(test))]
    if let Some(tpm_key) = derive_key_from_tpm() {
        set_evm_key(tpm_key);
        return tpm_key;
    }

    // Fallback to hardcoded key (no TPM available or in test mode)
    let fallback = *EVM_HMAC_KEY_FALLBACK;
    set_evm_key(fallback);
    fallback
}

/// Derive a 32-byte key from the TPM using get_random.
fn derive_key_from_tpm() -> Option<[u8; 32]> {
    const TPM_BASE_ADDR: u64 = 0xFED40000;
    let mut tpm = unsafe { crate::drivers::tpm::TpmDriver::new(TPM_BASE_ADDR) };

    if tpm.probe().is_err() {
        return None;
    }

    match tpm.get_random(32) {
        Ok(random_bytes) if random_bytes.len() >= 32 => {
            let mut key = [0u8; 32];
            key.copy_from_slice(&random_bytes[..32]);
            crate::serial::println!("[EVM] Key derived from TPM");
            Some(key)
        }
        _ => {
            crate::serial::println!("[EVM] TPM key derivation failed, using fallback");
            None
        }
    }
}

/// Compute an EVM HMAC-SHA256 over file metadata (inode, size, mtime).
pub fn evm_compute_hmac(inode: u64, size: u64, mtime: u64) -> [u8; 32] {
    let key = get_evm_key();
    let mut data = alloc::vec::Vec::new();
    data.extend_from_slice(&inode.to_le_bytes());
    data.extend_from_slice(&size.to_le_bytes());
    data.extend_from_slice(&mtime.to_le_bytes());
    hmac_sha256(&key, &data)
}

// ---------------------------------------------------------------------------
// Stack canary
// ---------------------------------------------------------------------------

/// Global random canary value, seeded once at boot.
static STACK_CANARY_VALUE: AtomicU64 = AtomicU64::new(0);

/// Generate and store the global stack canary value.
/// Uses RDRAND if available, otherwise a simple LCG fallback.
pub fn init_canary() {
    let canary = generate_random_u64();
    STACK_CANARY_VALUE.store(canary, Ordering::Release);
}

/// Return the current stack canary value.
pub fn canary_value() -> u64 {
    STACK_CANARY_VALUE.load(Ordering::Acquire)
}

/// Generate a 64-bit random value using RDRAND, with multi-source fallback.
fn generate_random_u64() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        // Try RDRAND up to 10 times
        for _ in 0..10 {
            let val: u64;
            let ok: u8;
            unsafe {
                core::arch::asm!(
                    "rdrand {0}",
                    "setc {1}",
                    out(reg) val,
                    out(reg_byte) ok,
                );
            }
            if ok != 0 && val != 0 {
                return val;
            }
        }
    }
    // Fallback: mix multiple entropy sources for non-deterministic canary.
    static FALLBACK_COUNTER: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    let counter = FALLBACK_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let tsc: u64 = {
        #[cfg(target_arch = "x86_64")]
        {
            let lo: u32;
            let hi: u32;
            unsafe {
                core::arch::asm!(
                    "rdtsc",
                    out("eax") lo,
                    out("edx") hi,
                );
            }
            ((hi as u64) << 32) | (lo as u64)
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            0
        }
    };
    // Mix with stack pointer address for per-process variation
    let stack_addr = &tsc as *const u64 as u64;
    // Use a simple but non-trivial mixing function
    let mut val = tsc.wrapping_add(stack_addr).wrapping_add(counter);
    // Ensure the canary has at least one set bit in high and low bytes
    // (prevents null-byte and 0xFF truncation attacks)
    val |= 0x0101_0000_0000_0101;
    val
}

/// Initialise IMA subsystem (init canary, seed log).
pub fn init() {
    init_canary();
    crate::serial::println!("[IMA] IMA/EVM subsystem initialised");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_empty() {
        let hash = sha256(b"");
        // Known SHA-256 of empty string
        let expected = [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14,
            0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
            0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c,
            0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
        ];
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_sha256_hello() {
        let hash = sha256(b"hello");
        let expected = [
            0x2c, 0xf2, 0x4d, 0xba, 0x5f, 0xb0, 0xa3, 0x0e,
            0x26, 0xe8, 0x3b, 0x2a, 0xc5, 0xb9, 0xe2, 0x9e,
            0x1b, 0x16, 0x1e, 0x5c, 0x1f, 0xa7, 0x42, 0x5e,
            0x73, 0x04, 0x33, 0x62, 0x93, 0x8b, 0x98, 0x24,
        ];
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_sha256_abc() {
        let hash = sha256(b"abc");
        let expected = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea,
            0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
            0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c,
            0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
        ];
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_sha256_abcdbcde() {
        let input = b"abcdbcde";
        let hash = sha256(input);
        // Verify by recomputing
        let hash2 = sha256(input);
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_sha256_multi_block() {
        // Input spanning 2 blocks (65 bytes)
        let data = b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"; // 65 A's
        let h = sha256(data);
        // Just verify it's deterministic and non-zero
        assert_ne!(h, [0u8; 32]);
        assert_eq!(sha256(data), sha256(data));

        // Input exactly 55 bytes (fits in 1 block with padding)
        let data55 = [0x41u8; 55];
        let h55 = sha256(&data55);
        assert_ne!(h55, [0u8; 32]);
        assert_eq!(h55, sha256(&data55));

        // Input exactly 56 bytes (padding extends to block 2)
        let data56 = [0x41u8; 56];
        let h56 = sha256(&data56);
        assert_ne!(h56, [0u8; 32]);
        assert_eq!(h56, sha256(&data56));

        // Input exactly 64 bytes (fills block exactly, needs padding block)
        let data64 = [0x41u8; 64];
        let h64 = sha256(&data64);
        assert_ne!(h64, [0u8; 32]);
        assert_eq!(h64, sha256(&data64));

        // Input exactly 128 bytes (2 full blocks + padding)
        let data128 = [0x41u8; 128];
        let h128 = sha256(&data128);
        assert_ne!(h128, [0u8; 32]);
        assert_eq!(h128, sha256(&data128));
    }

    #[test]
    fn test_sha256_single_byte() {
        let h = sha256(b"A");
        assert_ne!(h, [0u8; 32]);
        // Known: SHA256("A") = "559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd"
        assert_eq!(h[0], 0x55);
        assert_eq!(h[1], 0x9a);
        assert_eq!(h[31], 0xfd);
    }

    #[test]
    fn test_hmac_empty_key() {
        let data = b"test data";
        let h = hmac_sha256(b"", data);
        assert_ne!(h, [0u8; 32]);
        assert_eq!(h, hmac_sha256(b"", data)); // deterministic
    }

    #[test]
    fn test_ima_measurement_struct() {
        let m = ImaMeasurement {
            pcr: 10,
            hash: [0xAB; 32],
            path: b"/bin/test".to_vec(),
        };
        assert_eq!(m.pcr, 10);
        assert_eq!(m.hash[0], 0xAB);
        assert_eq!(&m.path[..], b"/bin/test");
    }

    #[test]
    fn test_evm_verify_different_args() {
        let inode = 42u64;
        let size = 1024u64;
        let mtime = 1700000000u64;
        let hmac = evm_compute_hmac(inode, size, mtime);
        assert!(evm_verify(inode, size, mtime, &hmac));
    }

    #[test]
    fn test_generate_random_u64_nonzero() {
        let v = generate_random_u64();
        assert_ne!(v, 0);
        // Calling generate_random_u64 multiple times should work
        let v2 = generate_random_u64();
        assert_ne!(v2, 0);
    }

    #[test]
    fn test_init_canary_sets_value() {
        // Reset canary, then init
        STACK_CANARY_VALUE.store(0, Ordering::SeqCst);
        init_canary();
        assert_ne!(canary_value(), 0);
    }

    #[test]
    fn test_sha256_deterministic() {
        let data = b"The quick brown fox jumps over the lazy dog";
        let h1 = sha256(data);
        let h2 = sha256(data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hmac_sha256_deterministic() {
        let key = b"key";
        let data = b"The quick brown fox jumps over the lazy dog";
        let h1 = hmac_sha256(key, data);
        let h2 = hmac_sha256(key, data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hmac_sha256_differs_with_diff_key() {
        let data = b"test data";
        let h1 = hmac_sha256(b"key1", data);
        let h2 = hmac_sha256(b"key2", data);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_measure_exec_adds_entry() {
        let _guard = crate::test_serial::acquire();
        // Reset log for clean state
        *IMA_LOG.lock() = Vec::new();
        let len_before = get_measurement_log().len();
        measure_exec(b"fake-elf-binary", "/bin/test");
        let log = get_measurement_log();
        assert_eq!(log.len(), len_before + 1);
        assert_eq!(&log.last().unwrap().path[..], b"/bin/test");
    }

    #[test]
    fn test_measure_exec_same_binary_same_hash() {
        let _guard = crate::test_serial::acquire();
        // Reset log for clean state
        *IMA_LOG.lock() = Vec::new();
        measure_exec(b"test-binary", "/bin/a");
        measure_exec(b"test-binary", "/bin/b");
        let log = get_measurement_log();
        let last_two = &log[log.len().saturating_sub(2)..];
        assert_eq!(last_two[0].hash, last_two[1].hash);
    }

    #[test]
    fn test_canary_generates_nonzero() {
        // Force reset of canary for testing
        STACK_CANARY_VALUE.store(0, Ordering::SeqCst);
        // Wait, init_canary() is not deterministic in tests.
        // Just verify the value is non-zero after init.
        if STACK_CANARY_VALUE.load(Ordering::Acquire) == 0 {
            init_canary();
        }
        assert_ne!(canary_value(), 0);
    }

    #[test]
    fn test_evm_verify_correct_metadata() {
        let inode = 42u64;
        let size = 1024u64;
        let mtime = 1700000000u64;
        let hmac = evm_compute_hmac(inode, size, mtime);
        assert!(evm_verify(inode, size, mtime, &hmac));
    }

    #[test]
    fn test_evm_verify_tampered_metadata() {
        let inode = 42u64;
        let size = 1024u64;
        let mtime = 1700000000u64;
        let hmac = evm_compute_hmac(inode, size, mtime);
        // Tamper with size
        assert!(!evm_verify(inode, 2048, mtime, &hmac));
    }

    #[test]
    fn test_ima_log_ring_buffer() {
        let _guard = crate::test_serial::acquire();
        // Fill the log past the max (reset it first for testing)
        *IMA_LOG.lock() = Vec::new();
        for i in 0..IMA_LOG_MAX + 100 {
            measure_exec(b"data", &alloc::format!("/bin/{}", i));
        }
        let log = get_measurement_log();
        assert_eq!(log.len(), IMA_LOG_MAX);
        // The first entry should be gone (evicted), last should remain
        assert_eq!(&log[0].path[..], b"/bin/100");
    }
}
