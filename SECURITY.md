# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.0.x   | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability in turnix, please report it responsibly.

**Email**: [uttam232002@gmail.com](mailto:uttam232002@gmail.com)
**Subject**: `[turnix Security] <brief description>`

### What to Include

- Type of vulnerability (e.g., buffer overflow, privilege escalation, race condition)
- Affected component and file paths
- Step-by-step reproduction instructions
- Proof-of-concept or exploit code (if applicable)
- Impact assessment (e.g., "allows arbitrary code execution in kernel mode")

### What to Expect

| Stage | Timeline |
| ----- | -------- |
| Acknowledgement | Within **48 hours** |
| Triage & initial assessment | Within **7 days** |
| Fix or mitigation | Depends on severity, typically **14-30 days** |

We will provide:

- Confirmation of the vulnerability (or clarification if not reproducible)
- Expected timeline for a fix
- Any interim mitigations or workarounds

## Disclosure Policy

- **Coordinated Disclosure**: We request reasonable time to address the issue before public disclosure. We aim for a **90-day disclosure window**.
- **Credit**: Reporters will be credited in the security advisory and release notes (unless anonymity is requested).
- **Safe Harbour**: We will not pursue legal action against researchers who follow this policy and act in good faith.

## Scope

**In scope**:

- Kernel code (`kernel/`)
- Shared libraries (`shared/`)
- Userland binaries (`userland/`)
- Build tooling (`tools/`)
- CI/CD pipelines (`.github/`)

**Out of scope**:

- Denial-of-service attacks against public instances
- Social engineering of maintainers
- Issues in third-party dependencies (report upstream)

## Security Considerations

As an early-stage OS project, turnix is **not yet suitable for production or security-sensitive environments**. The project is under active development and has not undergone formal security audits.

Key security features implemented:

- POSIX capabilities (64-bit capability sets)
- Seccomp-BPF system call filtering
- Linux Security Module (LSM) hooks with DAC
- IMA/EVM integrity measurement
- Stack canaries for corruption detection
- ASLR and KASLR randomization

These are best-effort implementations and may contain vulnerabilities.
