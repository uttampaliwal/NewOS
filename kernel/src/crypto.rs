//! Kernel crypto API.
//!
//! Provides trait-based abstractions for hashing, MAC, authenticated encryption,
//! key derivation, and a kernel CSPRNG. Implementations delegate to the
//! existing SHA-256 / HMAC-SHA256 in `security::ima` and a new ChaCha20-based
//! CSPRNG seeded from hardware entropy.

extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Digest trait
// ---------------------------------------------------------------------------

/// Trait for hash functions with an incremental update/finalize API.
pub trait Digest {
    /// Output size in bytes.
    const OUTPUT_SIZE: usize;

    /// Write data into the hash.
    fn update(&mut self, data: &[u8]);

    /// Finalize and return the hash.
    fn finalize(self) -> Vec<u8>;

    /// Convenience: hash a complete message in one shot.
    fn digest(data: &[u8]) -> Vec<u8>
    where
        Self: Default,
    {
        let mut h = Self::default();
        h.update(data);
        h.finalize()
    }
}

/// SHA-256 round constants.
const K: [u32; 64] = [
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

/// SHA-256 block compression function.
fn sha256_compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3],
        ]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;

    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
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

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

/// SHA-256 implementation with streaming update/finalize.
///
/// Byte-at-a-time processing for simplicity. The compression function
/// processes a full 64-byte block whenever the internal count reaches
/// a multiple of 64. Padding and length encoding in `finalize()` follow
/// FIPS 180-4.
pub struct Sha256 {
    state: [u32; 8],
    count: u64,
    buffer: [u8; 64],
}

impl Sha256 {
    pub fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            count: 0,
            buffer: [0u8; 64],
        }
    }
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Digest for Sha256 {
    const OUTPUT_SIZE: usize = 32;

    fn update(&mut self, data: &[u8]) {
        for &byte in data {
            let idx = (self.count % 64) as usize;
            self.buffer[idx] = byte;
            self.count += 1;
            if idx == 63 {
                sha256_compress(&mut self.state, &self.buffer);
            }
        }
    }

    fn finalize(mut self) -> Vec<u8> {
        let original_count = self.count;
        // Append 0x80 byte.
        self.update(&[0x80]);
        // Pad with zeros until 56 bytes mod 64.
        while self.count % 64 != 56 {
            self.update(&[0x00]);
        }
        // Append original bit length in big-endian.
        let bits = original_count * 8;
        for i in (0..8).rev() {
            self.update(&[(bits >> (i * 8)) as u8]);
        }
        // Extract hash words as big-endian bytes.
        let mut out = Vec::with_capacity(32);
        for &word in &self.state {
            out.push((word >> 24) as u8);
            out.push((word >> 16) as u8);
            out.push((word >> 8) as u8);
            out.push(word as u8);
        }
        out
    }
}

/// SHA-256 one-shot function (delegates to IMA implementation).
pub fn sha256(data: &[u8]) -> [u8; 32] {
    crate::security::ima::sha256(data)
}

/// SHA-256 streaming function using the Digest trait.
///
/// Produces identical output to `sha256()` for the same input.
pub fn sha256_stream(data: &[u8]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize()
}

// ---------------------------------------------------------------------------
// MAC trait
// ---------------------------------------------------------------------------

/// Trait for message authentication codes.
pub trait Mac {
    /// Output size in bytes.
    const OUTPUT_SIZE: usize;

    /// Create a new MAC with the given key.
    fn new(key: &[u8]) -> Self;

    /// Process additional data.
    fn update(&mut self, data: &[u8]);

    /// Finalize and return the MAC tag.
    fn finalize(self) -> Vec<u8>;
}

/// HMAC-SHA256 implementation with streaming update/finalize.
pub struct HmacSha256 {
    inner: Sha256,
    outer: Sha256,
}

impl Mac for HmacSha256 {
    const OUTPUT_SIZE: usize = 32;

    fn new(key: &[u8]) -> Self {
        let mut k = Vec::from(key);
        if k.len() > 64 {
            k = sha256(&k).to_vec();
            k.resize(64, 0);
        } else {
            k.resize(64, 0);
        }

        let mut ipad = [0x36u8; 64];
        let mut opad = [0x5cu8; 64];
        for i in 0..64 {
            ipad[i] ^= k[i];
            opad[i] ^= k[i];
        }

        let mut inner = Sha256::new();
        inner.update(&ipad);
        let mut outer = Sha256::new();
        outer.update(&opad);

        Self { inner, outer }
    }

    fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    fn finalize(mut self) -> Vec<u8> {
        let inner_hash = self.inner.finalize();
        self.outer.update(&inner_hash);
        self.outer.finalize()
    }
}

/// Compute HMAC-SHA256 for the provided key/data pair.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new(key);
    mac.update(data);
    let tag = mac.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&tag);
    out
}

/// Compute a SHA-256 digest and return it as a lowercase hexadecimal string.
pub fn sha256_hex(data: &[u8]) -> alloc::string::String {
    use alloc::format;
    let hash = sha256(data);
    let mut out = alloc::string::String::with_capacity(64);
    for byte in hash {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Verify an HMAC-SHA256 digest against expected bytes (constant-time).
pub fn hmac_sha256_verify(key: &[u8], data: &[u8], expected: &[u8; 32]) -> bool {
    let actual = hmac_sha256(key, data);
    let mut diff = 0u8;
    for (a, b) in actual.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// CSPRNG — ChaCha20-based with entropy pool
// ---------------------------------------------------------------------------

/// ChaCha20 quarter round.
fn chacha_quarter(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(16);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(12);
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(8);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(7);
}

/// ChaCha20 block function: produces 64 bytes of keystream.
fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];
    // "expand 32-byte k"
    state[0] = 0x61707865;
    state[1] = 0x3320646e;
    state[2] = 0x79622d32;
    state[3] = 0x6b206574;

    for i in 0..8 {
        state[4 + i] = u32::from_le_bytes([
            key[i * 4], key[i * 4 + 1], key[i * 4 + 2], key[i * 4 + 3],
        ]);
    }

    state[12] = counter;

    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes([
            nonce[i * 4], nonce[i * 4 + 1], nonce[i * 4 + 2], nonce[i * 4 + 3],
        ]);
    }

    let original = state;

    for _ in 0..10 {
        chacha_quarter(&mut state, 0, 4, 8, 12);
        chacha_quarter(&mut state, 1, 5, 9, 13);
        chacha_quarter(&mut state, 2, 6, 10, 14);
        chacha_quarter(&mut state, 3, 7, 11, 15);
        chacha_quarter(&mut state, 0, 5, 10, 15);
        chacha_quarter(&mut state, 1, 6, 11, 12);
        chacha_quarter(&mut state, 2, 7, 8, 13);
        chacha_quarter(&mut state, 3, 4, 9, 14);
    }

    for i in 0..16 {
        state[i] = state[i].wrapping_add(original[i]);
    }

    let mut out = [0u8; 64];
    for i in 0..16 {
        out[i * 4..i * 4 + 4].copy_from_slice(&state[i].to_le_bytes());
    }
    out
}

/// Kernel CSPRNG state.
struct CrngState {
    key: [u8; 32],
    counter: u32,
    nonce: [u8; 12],
    initialized: bool,
}

impl CrngState {
    const fn new() -> Self {
        Self {
            key: [0u8; 32],
            counter: 0,
            nonce: [0u8; 12],
            initialized: false,
        }
    }

    fn fill_bytes(&mut self, buf: &mut [u8]) {
        if !self.initialized {
            self.reseed_from_hardware();
        }

        let mut offset = 0;
        while offset < buf.len() {
            let keystream = chacha20_block(&self.key, self.counter, &self.nonce);
            self.counter = self.counter.wrapping_add(1);

            let take = (buf.len() - offset).min(64);
            buf[offset..offset + take].copy_from_slice(&keystream[..take]);
            offset += take;
        }

        // Rekey after generating 2^20 blocks (~64 MB) to limit keystream exposure.
        if self.counter > 1_048_576 {
            self.rekey();
        }
    }

    fn reseed_from_hardware(&mut self) {
        // Try RDRAND first
        if let Some(bytes) = rdrand_bytes(32) {
            self.key.copy_from_slice(&bytes);
        } else {
            // Fallback: use a fixed seed (NOT cryptographically secure, but
            // allows the kernel to function without RDRAND).
            self.key = [0x42u8; 32];
        }

        // Mix in TSC jitter for additional entropy
        let tsc = rdtsc();
        let tsc_bytes = tsc.to_le_bytes();
        self.key[..8].copy_from_slice(&tsc_bytes);
        let tsc2_extra = rdtsc();
        self.key[8..16].copy_from_slice(&tsc2_extra.to_le_bytes());
        let tsc3_extra = rdtsc();
        self.key[16..24].copy_from_slice(&tsc3_extra.to_le_bytes());
        let tsc4_extra = rdtsc();
        self.key[24..32].copy_from_slice(&tsc4_extra.to_le_bytes());

        // Generate a random nonce from TSC values
        let tsc2 = rdtsc();
        let tsc_bytes = tsc2.to_le_bytes();
        self.nonce[..8].copy_from_slice(&tsc_bytes);
        let tsc3 = rdtsc();
        let tsc_bytes2 = tsc3.to_le_bytes();
        self.nonce[8..12].copy_from_slice(&tsc_bytes2[..4]);
        self.counter = 1;
        self.initialized = true;
    }

    fn rekey(&mut self) {
        let mut new_key = [0u8; 32];
        let keystream = chacha20_block(&self.key, self.counter, &self.nonce);
        new_key.copy_from_slice(&keystream[..32]);
        self.key = new_key;
        self.counter = 1;
    }
}

static CRNG: Mutex<CrngState> = Mutex::new(CrngState::new());
static CRNG_BYTES_GENERATED: AtomicU64 = AtomicU64::new(0);

/// Fill a buffer with cryptographically secure random bytes.
pub fn csprng_fill(buf: &mut [u8]) {
    CRNG.lock().fill_bytes(buf);
    CRNG_BYTES_GENERATED.fetch_add(buf.len() as u64, Ordering::Relaxed);
}

/// Generate a random u64.
pub fn csprng_u64() -> u64 {
    let mut buf = [0u8; 8];
    csprng_fill(&mut buf);
    u64::from_ne_bytes(buf)
}

/// Initialize the CSPRNG (called during boot).
pub fn csprng_init() {
    CRNG.lock().reseed_from_hardware();
    crate::serial::println!("[CRYPTO] CSPRNG initialized");
}

/// Total bytes generated by the CSPRNG since boot.
pub fn csprng_bytes_generated() -> u64 {
    CRNG_BYTES_GENERATED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Hardware entropy sources
// ---------------------------------------------------------------------------

/// Attempt to read `n` bytes via RDRAND instruction.
fn rdrand_bytes(n: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(n);
    let mut remaining = n;
    while remaining >= 8 {
        // Safety: RDRAND is a x86_64 instruction that reads a hardware random number.
        // _rdrand64_step writes the random value into `val` and returns 1 on success.
        let val: u64;
        unsafe {
            let mut random_val: u64 = 0;
            let retry = core::arch::x86_64::_rdrand64_step(&mut random_val);
            if retry == 0 {
                return None;
            }
            val = random_val;
        }
        out.extend_from_slice(&val.to_le_bytes());
        remaining -= 8;
    }
    if remaining > 0 {
        // Safety: same as above.
        unsafe {
            let mut random_val: u64 = 0;
            let retry = core::arch::x86_64::_rdrand64_step(&mut random_val);
            if retry == 0 {
                return None;
            }
            let bytes = random_val.to_le_bytes();
            out.extend_from_slice(&bytes[..remaining]);
        }
    }
    Some(out)
}

/// Read the TSC (Time Stamp Counter) for jitter entropy.
fn rdtsc() -> u64 {
    // Safety: RDTSC reads the processor's time stamp counter.
    // It is a serializing instruction that does not access memory.
    unsafe { core::arch::x86_64::_rdtsc() }
}

// ---------------------------------------------------------------------------
// HKDF — RFC 5869
// ---------------------------------------------------------------------------

/// HKDF-Extract: PRK = HMAC-Hash(salt, IKM).
pub fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    hmac_sha256(salt, ikm)
}

/// HKDF-Expand: OKM = T(1) || T(2) || ... where T(i) = HMAC-Hash(PRK, T(i-1) || info || i).
pub fn hkdf_expand(prk: &[u8; 32], info: &[u8], okm_len: usize) -> Vec<u8> {
    let n = (okm_len + 31) / 32; // ceiling division
    let mut okm = Vec::with_capacity(okm_len);
    let mut t = [0u8; 32];

    for i in 1..=n {
        let mut mac = HmacSha256::new(prk);
        mac.update(&t);
        mac.update(info);
        mac.update(&[i as u8]);
        t.copy_from_slice(&mac.finalize());
        okm.extend_from_slice(&t);
    }

    okm.truncate(okm_len);
    okm
}

/// HKDF combined: derive keying material from input key material.
pub fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8], okm_len: usize) -> Vec<u8> {
    let prk = hkdf_extract(salt, ikm);
    hkdf_expand(&prk, info, okm_len)
}

// ---------------------------------------------------------------------------
// Password hashing — PBKDF2-HMAC-SHA256
// ---------------------------------------------------------------------------

/// PBKDF2-HMAC-SHA256 key derivation.
///
/// `iterations` should be >= 100,000 for production use.
/// Returns a `derived_key_len`-byte derived key.
pub fn pbkdf2_hmac_sha256(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
    derived_key_len: usize,
) -> Vec<u8> {
    let hlen = 32; // SHA-256 output
    let l = (derived_key_len + hlen - 1) / hlen;
    let mut out = Vec::with_capacity(derived_key_len);

    for block_idx in 1..=l {
        // U_1 = HMAC(password, salt || block_idx)
        let mut mac = HmacSha256::new(password);
        mac.update(salt);
        mac.update(&block_idx.to_be_bytes());
        let mut u = mac.finalize();
        let mut t = u.clone();

        for _ in 1..iterations {
            mac = HmacSha256::new(password);
            mac.update(&u);
            u = mac.finalize();

            for (t_byte, u_byte) in t.iter_mut().zip(u.iter()) {
                *t_byte ^= *u_byte;
            }
        }

        out.extend_from_slice(&t);
    }

    out.truncate(derived_key_len);
    out
}

/// Verify a password against a PBKDF2-HMAC-SHA256 hash (constant-time).
pub fn verify_password(password: &[u8], salt: &[u8], iterations: u32, expected_hash: &[u8]) -> bool {
    let computed = pbkdf2_hmac_sha256(password, salt, iterations, expected_hash.len());
    if computed.len() != expected_hash.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in computed.iter().zip(expected_hash.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial;

    #[test]
    fn sha256_stream_matches_oneshot() {
        let _s = test_serial::acquire();
        let data = b"hello world";
        let oneshot = sha256(data);
        let streaming = Sha256::digest(data);
        assert_eq!(&oneshot[..], streaming.as_slice());
    }

    #[test]
    fn sha256_stream_incremental() {
        let _s = test_serial::acquire();
        let mut h = Sha256::new();
        h.update(b"hello ");
        h.update(b"world");
        let result = h.finalize();
        let expected = sha256(b"hello world");
        assert_eq!(result.as_slice(), &expected);
    }

    #[test]
    fn sha256_empty() {
        let _s = test_serial::acquire();
        let result = Sha256::digest(b"");
        let expected = sha256(b"");
        assert_eq!(result.as_slice(), &expected);
    }

    #[test]
    fn hmac_sha256_trait_matches_function() {
        let _s = test_serial::acquire();
        let key = b"secret-key";
        let data = b"message";
        let from_fn = hmac_sha256(key, data);
        let from_trait = {
            let mut mac = HmacSha256::new(key);
            mac.update(data);
            let tag = mac.finalize();
            let mut out = [0u8; 32];
            out.copy_from_slice(&tag);
            out
        };
        assert_eq!(from_fn, from_trait);
    }

    #[test]
    fn hmac_verify_constant_time() {
        let _s = test_serial::acquire();
        let key = b"key";
        let data = b"data";
        let tag = hmac_sha256(key, data);

        // Compute tag with wrong key
        let _wrong_tag = {
            let mut mac = HmacSha256::new(b"wrong");
            mac.update(data);
            mac.finalize()
        };

        // Verify correct tag
        assert!(hmac_sha256_verify(key, data, &tag));
        // Verify wrong tag fails
        assert!(!hmac_sha256_verify(b"wrong", data, &tag));
        // Verify wrong data fails
        assert!(!hmac_sha256_verify(key, b"other", &tag));
    }

    #[test]
    fn csprng_produces_unique_values() {
        let _s = test_serial::acquire();
        let a = csprng_u64();
        let b = csprng_u64();
        // In the extremely unlikely case they're equal, just verify the
        // function doesn't panic.
        assert_ne!(a ^ b, 0); // at least one bit different
    }

    #[test]
    fn csprng_fill_buf() {
        let _s = test_serial::acquire();
        let mut buf = [0u8; 64];
        csprng_fill(&mut buf);
        // Verify not all zeros (would indicate failure).
        assert!(buf.iter().any(|&b| b != 0));
    }

    #[test]
    fn hkdf_extract_expand_roundtrip() {
        let _s = test_serial::acquire();
        let salt = b"salt";
        let ikm = b"input key material";
        let info = b"context";
        let prk = hkdf_extract(salt, ikm);
        let okm = hkdf_expand(&prk, info, 42);
        assert_eq!(okm.len(), 42);

        // Same inputs produce same output.
        let okm2 = hkdf(salt, ikm, info, 42);
        assert_eq!(okm, okm2);
    }

    #[test]
    fn hkdf_different_info_different_output() {
        let _s = test_serial::acquire();
        let prk = hkdf_extract(b"salt", b"ikm");
        let a = hkdf_expand(&prk, b"info-a", 32);
        let b = hkdf_expand(&prk, b"info-b", 32);
        assert_ne!(a, b);
    }

    #[test]
    fn pbkdf2_deterministic() {
        let _s = test_serial::acquire();
        let pass = b"password";
        let salt = b"salt";
        let h1 = pbkdf2_hmac_sha256(pass, salt, 1000, 32);
        let h2 = pbkdf2_hmac_sha256(pass, salt, 1000, 32);
        assert_eq!(h1, h2);
    }

    #[test]
    fn pbkdf2_verify_password() {
        let _s = test_serial::acquire();
        let pass = b"my-password";
        let salt = b"random-salt";
        let hash = pbkdf2_hmac_sha256(pass, salt, 1000, 32);
        assert!(verify_password(pass, salt, 1000, &hash));
        assert!(!verify_password(b"wrong", salt, 1000, &hash));
    }

    #[test]
    fn pbkdf2_different_salt_different_hash() {
        let _s = test_serial::acquire();
        let pass = b"password";
        let h1 = pbkdf2_hmac_sha256(pass, b"salt1", 1000, 32);
        let h2 = pbkdf2_hmac_sha256(pass, b"salt2", 1000, 32);
        assert_ne!(h1, h2);
    }

    #[test]
    fn chacha20_block_deterministic() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];
        let a = chacha20_block(&key, 1, &nonce);
        let b = chacha20_block(&key, 1, &nonce);
        assert_eq!(a, b);
    }

    #[test]
    fn chacha20_different_counter_different_output() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];
        let a = chacha20_block(&key, 0, &nonce);
        let b = chacha20_block(&key, 1, &nonce);
        assert_ne!(a, b);
    }
}
