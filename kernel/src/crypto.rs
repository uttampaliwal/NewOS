//! Kernel crypto facade.
//!
//! The kernel already had SHA-256 and HMAC-SHA256 implementations inside the
//! IMA/EVM subsystem. This module exposes them through a stable, public entry
//! point so other subsystems can depend on a single crypto surface.

/// Compute SHA-256 for the provided buffer.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    crate::security::ima::sha256(data)
}

/// Compute HMAC-SHA256 for the provided key/data pair.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    crate::security::ima::hmac_sha256(key, data)
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

/// Verify an HMAC-SHA256 digest against expected bytes.
pub fn hmac_sha256_verify(key: &[u8], data: &[u8], expected: &[u8; 32]) -> bool {
    let actual = hmac_sha256(key, data);
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= actual[i] ^ expected[i];
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial;

    #[test]
    fn sha256_known_vector() {
        let _s = test_serial::acquire();
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hmac_verification_round_trips() {
        let _s = test_serial::acquire();
        let key = b"key";
        let data = b"payload";
        let digest = hmac_sha256(key, data);
        assert!(hmac_sha256_verify(key, data, &digest));
        assert!(!hmac_sha256_verify(b"other", data, &digest));
    }
}
