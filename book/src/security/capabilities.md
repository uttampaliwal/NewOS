# Capabilities

Turnix implements POSIX.1e capabilities for fine-grained privilege control.

## Capability Sets

- **Permitted**: Privileges the process may use
- **Effective**: Privileges currently active
- **Inheritable**: Preserved across exec
- **Bounding**: Maximum capabilities after exec
- **Ambient**: Inherited by non-privileged exec

## Key Operations

- `prctl(PR_CAP_AMBIENT)` — manage ambient capabilities
- File capabilities via extended attributes
- Capability dropping via `PR_CAPBSET_DROP`

For full details, see `docs/security/capabilities.md`.
