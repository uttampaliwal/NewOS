> For detailed reference documentation, see [docs/security/](../../../docs/security/)

# Threat Model

Turnix defines clear trust boundaries between kernel and user space.

## Trust Boundaries

- **Ring 0 (Kernel)**: Full hardware access, trusted
- **Ring 3 (User)**: Untrusted, restricted via capabilities and seccomp
- **Boot chain**: UEFI Secure Boot → signed loader → verified kernel

## Attacker Models

- Malicious userspace applications
- Compromised system services
- Physical access (TPM-backed key storage)
- Network-based attacks (future)

## TCB (Trusted Computing Base)

The kernel, boot loader, and TPM firmware form the TCB. All security decisions
(capabilities, seccomp, LSM hooks) are enforced within the TCB.

For full details, see `docs/security/threat-model.md`.
