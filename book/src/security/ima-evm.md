> For detailed reference documentation, see [docs/security/](../../../docs/security/)

# IMA/EVM

Integrity Measurement Architecture (IMA) and Extended Verification Module (EVM)
provide file integrity verification.

## IMA

IMA measures file hashes at runtime and stores them in a kernel log ring
buffer. Measurements can be extended to TPM PCRs for remote attestation.

## EVM

EVM verifies file metadata integrity using HMAC-SHA256. The HMAC key
is derived from TPM 2.0 when available, falling back to a software key.

## TPM Integration

TPM 2.0 TIS driver provides:
- `TPM2_CC_GET_RANDOM`: Key derivation
- Seal/unseal: Secure key storage
- PCR extension: Boot chain measurement

For full details, see `docs/security/ima-evm.md`.
