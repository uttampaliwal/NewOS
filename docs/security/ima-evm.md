# Integrity Measurement Architecture (IMA) & Extended Verification Module (EVM)

Turnix implements IMA for binary measurement and EVM for file metadata
integrity verification, providing a complete integrity chain from boot.

---

## Architecture

```
Binary Execution
    │
    ▼
┌─────────────────────┐
│    IMA Measurement   │
│  SHA-256(elf_data)  │
│  Append to ring buf │
└─────────────────────┘
    │
    ▼
┌─────────────────────┐
│    EVM Verification  │
│  HMAC(inode, size,   │
│       mtime, key)   │
│  Compare vs stored   │
└─────────────────────┘
    │
    ▼
┌─────────────────────┐
│    TPM 2.0 Storage   │
│  Seal/unseal keys    │
│  Hardware RNG        │
└─────────────────────┘
```

---

## IMA (Integrity Measurement Architecture)

### How It Works

Every time a binary is executed, IMA computes a SHA-256 hash of the ELF
data and appends it to a ring buffer:

```rust
pub fn measure_exec(elf_data: &[u8], path: &str) {
    let hash = sha256(elf_data);
    let entry = ImaMeasurement {
        pcr_index: 10,          // PCR 10 for runtime measurements
        hash,                   // 32-byte SHA-256
        path: path.to_string(), // File path
    };
    // Append to 4096-entry ring buffer
    MEASUREMENT_LOG.lock().push(entry);
}
```

### Properties

- **PCR Index 10**: Runtime measurements (separate from boot measurements)
- **Ring Buffer**: 4096 entries, oldest entries are overwritten
- **Audit Trail**: `get_measurement_log()` returns the full history
- **No Blocking**: Measurement is non-blocking and does not prevent execution

### Use Cases

- Detect if a tampered binary was executed
- Verify system integrity after compromise
- Compliance logging for security audits

---

## EVM (Extended Verification Module)

### How It Works

EVM computes an HMAC-SHA256 over file metadata and verifies it against
the stored value:

```rust
pub fn evm_compute_hmac(inode: u64, size: u64, mtime: u64) -> [u8; 32] {
    let key = *EVM_HMAC_KEY.lock();
    let mut data = Vec::new();
    data.extend_from_slice(&inode.to_le_bytes());
    data.extend_from_slice(&size.to_le_bytes());
    data.extend_from_slice(&mtime.to_le_bytes());
    hmac_sha256(&key, &data)
}
```

### Verification Flow

1. Read stored HMAC from file's `security.evm` extended attribute
2. Compute HMAC over current `(inode, size, mtime)`
3. Compare using **constant-time comparison** (prevents timing attacks)
4. Return `Ok(())` if match, `Err(LsmError::AccessDenied)` if mismatch

### Properties

- **Metadata Integrity**: Detects unauthorized changes to inode size, mtime
- **Timing-Safe**: Constant-time comparison prevents side-channel attacks
- **Key Management**: EVM key derived from TPM or hardcoded fallback

---

## SHA-256 Implementation

Turnix includes a complete FIPS 180-4 SHA-256 implementation:

```rust
pub fn sha256(data: &[u8]) -> [u8; 32] {
    // Full 64-round compression
    // Message schedule: W[0..63]
    // Working variables: a, b, c, d, e, f, g, h
    // Hash values: H[0..7] (initial: 6a09e667, bb67ae85, ...)
}
```

No external crypto libraries required — fully self-contained.

---

## HMAC-SHA256

```rust
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    // Inner pad: key XOR 0x36
    // Outer pad: key XOR 0x5c
    // HMAC = SHA256(outer_pad || SHA256(inner_pad || data))
}
```

---

## TPM 2.0 Integration

### Key Derivation

```rust
fn derive_key_from_tpm() -> Option<[u8; 32]> {
    let mut tpm = TpmDriver::new(0xFED40000);
    tpm.probe()?;
    let random_bytes = tpm.get_random(32)?;
    Some(random_bytes[..32].try_into().unwrap())
}
```

### Fallback

When TPM is unavailable (test environments, emulated systems):
- EVM uses a hardcoded 32-byte key
- Key is stored in `Mutex<Option<[u8; 32]>>`
- Lazy initialization on first use

### Future Enhancements

- TPM2_CC_SEAL for sealing keys to PCR state
- TPM2_CC_UNSEAL for remote attestation
- Secure boot chain verification

---

## Security Properties

| Property | Mechanism | Guarantee |
|----------|-----------|-----------|
| Binary integrity | IMA SHA-256 measurement | Tampered binaries are detectable |
| Metadata integrity | EVM HMAC verification | Unauthorized inode changes are detected |
| Timing safety | Constant-time comparison | No timing side-channel attacks |
| Key security | TPM hardware random | EVM key derived from hardware RNG |
| Audit trail | 4096-entry ring buffer | Full history of executed binaries |
