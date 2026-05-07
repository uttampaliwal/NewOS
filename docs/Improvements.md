\### Turnix Implementation Playbook and One‑Stop Roadmap



\*\*This single file consolidates the gap analysis, prioritized improvements, concrete PRs, CI artifacts, templates, and milestone checklists needed to move Turnix from research to a reproducible, testable, contributor‑friendly OS project.\*\*



> “This report provides a comprehensive gap analysis of the Turnix codebase against modern production-grade Linux distributions (Arch Linux, Ubuntu LTS, Fedora, Debian). Based on the audit of existing branches (including `development`, `unstable`, `feat/advanced-vfs`, `feat/network-stack`, `feat/security`, etc.), tags, and the current Rust-based architecture, this document outlines a structured, multi-quarter roadmap to elevate Turnix to a production-ready state.”



\---



\## 1 Executive Summary



\*\*Goal:\*\* Turn the existing Turnix repository into a reproducible, testable, and contributor-friendly OS project by implementing a prioritized set of engineering, QA, CI, documentation, and governance changes.  

\*\*Outcome:\*\* A single canonical implementation plan with ready-to-apply PR artifacts (CI workflows, QEMU smoke test, PR templates, contributor docs, driver skeletons, and milestone checklists).



\---



\## 2 Top Priorities and First Deliverables



| \*\*Priority\*\* | \*\*Area\*\* | \*\*First deliverable\*\* |

|---:|---|---|

| \*\*P0\*\* | CI and reproducible builds | GitHub Actions workflow with build matrix and QEMU smoke job |

| \*\*P0\*\* | QEMU integration tests | `xtask test-qemu` harness and `scripts/qemu-smoke.sh` |

| \*\*P0\*\* | Driver framework | `drivers/device\_manager` + `drivers/kobject` skeleton crate |

| \*\*P1\*\* | Memory subsystem | `mm/` demand paging PR + memstress harness |

| \*\*P1\*\* | Testing and fuzzing | `cargo-llvm-cov` integration and `cargo-fuzz` targets |

| \*\*P2\*\* | Userspace and packaging | `userspace/init` + `packaging/` reproducible recipe |

| \*\*P2\*\* | Security hardening | `security/` skeleton and W^X/KASLR enforcement checks |



\---



\## 3 Repository Layout and Branching Rules



\*\*Repository layout\*\* (apply as a migration PR that moves files and updates build scripts):



```

turnix/

├── arch/

├── kernel/

├── mm/

├── fs/

├── drivers/

├── include/

├── security/

├── net/

├── userspace/

│   ├── init/

│   ├── libturnix/

│   ├── shell/

│   └── coreutils/

├── packaging/

├── scripts/

├── docs/

├── tools/

└── .github/workflows/

```



\*\*Branching rules\*\* to add to `CONTRIBUTING.md` and enforce via branch protection:

\- `main` protected; releases only via automated pipeline.

\- `turnix-next` integration branch rebuilt nightly.

\- Topic branches prefixed by subsystem: `fs/`, `net/`, `core/`, `drivers/`.

\- Signed commits required for `turnix-next` and `main`.

\- PRs must be rebased on `turnix-next` before merge.



\---



\## 4 Detailed Action Plan by Area



\### CI and Reproducible Builds

\*\*Objectives\*\*

\- Fast, deterministic verification for PRs.

\- Reproducible artifacts for releases.



\*\*Actions\*\*

\- Add `rust-toolchain.toml` pinning toolchain versions.

\- Create `.github/workflows/ci.yml` with jobs:

&#x20; - `format` (`cargo fmt -- --check`)

&#x20; - `lint` (`cargo clippy -- -D warnings`)

&#x20; - `build` (workspace release)

&#x20; - `unit-tests` (`cargo test --workspace`)

&#x20; - `qemu-smoke` (boots kernel and asserts banner)

\- Cache `\~/.cargo/registry`, `\~/.cargo/git`, and `target/`.

\- Set `SOURCE\_DATE\_EPOCH` and strip timestamps in build artifacts.

\- Publish `docs/reproducible.md` with build checklist.



\*\*Deliverable\*\*

\- PR: `.github/workflows/ci.yml`, `rust-toolchain.toml`, `docs/reproducible.md`.



\---



\### QEMU Integration Tests and xtask

\*\*Objectives\*\*

\- Prevent regressions in boot and driver stacks.

\- Provide deterministic CI failure signals.



\*\*Actions\*\*

\- Implement `cargo xtask test-qemu` that:

&#x20; - Builds kernel and minimal init image.

&#x20; - Launches QEMU with `-serial stdio -no-reboot`.

&#x20; - Waits for deterministic banner `TURNIX\_OK` and checks exit code.

\- Add `scripts/qemu-smoke.sh` for local runs and CI.

\- Add a CI job that runs `xtask test-qemu` with a 60s timeout.



\*\*Deliverable\*\*

\- PR: `xtask` additions, `scripts/qemu-smoke.sh`, `.github/workflows/qemu-smoke.yml`.



\---



\### Driver Framework and Device Model

\*\*Objectives\*\*

\- Typed device lifecycle, safe concurrency, and sysfs-like visibility.



\*\*Actions\*\*

\- Add `drivers/device\_manager` crate with `DeviceDriver` trait:

&#x20; ```rust

&#x20; pub trait DeviceDriver: Send + Sync {

&#x20;     fn probe(\&self, device\_info: \&DeviceInfo) -> Result<(), DriverError>;

&#x20;     fn initialize(\&mut self) -> Result<(), DriverError>;

&#x20;     fn suspend(\&mut self);

&#x20;     fn resume(\&mut self);

&#x20; }

&#x20; ```

\- Add `drivers/kobject` crate implementing a small `/sys/turnix/` tree.

\- Provide unit tests that simulate PCI/virtio device registration.

\- Document ABI stability rules for future dynamic modules.



\*\*Deliverable\*\*

\- PR: `drivers/` skeleton, tests, and docs.



\---



\### Memory Management and Stability

\*\*Objectives\*\*

\- Demand paging, page cache, OOM handling, and W^X enforcement.



\*\*Actions\*\*

\- Implement demand paging and a page cache in `mm/`.

\- Add OOM killer policy and logging hooks.

\- Enforce W^X in `mm/paging` and add KASLR seed at boot.

\- Add CI check that scans `unsafe` blocks for `// SAFETY:` comments.



\*\*Deliverable\*\*

\- PR: `mm/` demand paging, memstress harness, `scripts/memstress.sh`.



\---



\### Testing, Coverage, and Fuzzing

\*\*Objectives\*\*

\- Detect UB and logic bugs early; measure coverage.



\*\*Actions\*\*

\- Integrate `cargo-llvm-cov` and publish coverage artifacts.

\- Add `cargo-fuzz` targets for syscall handlers, VFS, and network stack.

\- Run `Miri` on `libturnix` and isolated kernel logic where possible.

\- Add nightly fuzzing and coverage jobs in CI.



\*\*Deliverable\*\*

\- PR: `.github/workflows/fuzz-and-coverage.yml`, `tools/fuzz/` targets.



\---



\### Userspace, VFS, and Packaging

\*\*Objectives\*\*

\- Usable POSIX-like environment and reproducible package system.



\*\*Actions\*\*

\- Merge `feat/advanced-vfs` and add journaling layer plan.

\- Stabilize `libturnix` and provide POSIX compatibility shims.

\- Ensure `init` spawns `shell` and coreutils; add userspace smoke tests.

\- Design minimal package format and reproducible build recipes in `packaging/`.



\*\*Deliverable\*\*

\- PR: `userspace/` smoke tests, `packaging/` recipe examples.



\---



\### Security and Hardening

\*\*Objectives\*\*

\- W^X, KASLR, secure syscall boundaries, and a pluggable MAC.



\*\*Actions\*\*

\- Add `security/` skeleton with capability model and seccomp-like filters.

\- Enforce `copy\_from\_user`/`copy\_to\_user` wrappers with bounds checks.

\- Add `SECURITY.md` and a disclosure process.

\- Add security CI job for static analysis and SBOM generation.



\*\*Deliverable\*\*

\- PR: `security/` skeleton, `SECURITY.md`, `.github/workflows/security-scan.yml`.



\---



\## 5 PR Templates, CONTRIBUTING, and Checklists



\### PULL\_REQUEST\_TEMPLATE.md

```markdown

\## Summary

Short description of the change.



\## Related issues

Fixes: #<issue-id>



\## Checklist

\- \[ ] I ran `cargo fmt` and `cargo clippy` locally.

\- \[ ] Tests added or updated.

\- \[ ] Docs updated in `docs/`.

\- \[ ] `// SAFETY:` comment added for every `unsafe` block.

\- \[ ] CI passes on `turnix-next`.



\## Testing notes

How to run the tests and smoke QEMU test locally.

```



\### CONTRIBUTING.md (key excerpts)

\- \*\*Toolchain\*\*: `rustup toolchain install stable-<date>`; `rustup component add rustfmt clippy`.

\- \*\*Local QEMU\*\*: `scripts/qemu-smoke.sh` usage example.

\- \*\*Branching\*\*: topic branch naming rules and PR process.

\- \*\*Commit format\*\*: `<subsystem>: <short description>` with `Signed-off-by`.



\### PR Review Checklist for Kernel Changes

\- Two approvals required for `kernel/` and `mm/`.

\- `unsafe` blocks must include `// SAFETY:` with invariants.

\- Unit tests or QEMU smoke test added for behavioral changes.



\---



\## 6 CI Artifacts and Example Files



\### Example GitHub Actions CI snippet (ci.yml)

```yaml

name: CI



on:

&#x20; pull\_request:

&#x20;   branches: \[ turnix-next ]

&#x20; push:

&#x20;   branches: \[ turnix-next ]



jobs:

&#x20; format:

&#x20;   runs-on: ubuntu-latest

&#x20;   steps:

&#x20;     - uses: actions/checkout@v4

&#x20;     - uses: actions-rs/toolchain@v1

&#x20;       with:

&#x20;         toolchain: stable

&#x20;     - run: cargo fmt -- --check



&#x20; build:

&#x20;   runs-on: ubuntu-latest

&#x20;   needs: format

&#x20;   steps:

&#x20;     - uses: actions/checkout@v4

&#x20;     - uses: actions/cache@v4

&#x20;       with:

&#x20;         path: |

&#x20;           \~/.cargo/registry

&#x20;           \~/.cargo/git

&#x20;           target

&#x20;         key: ${{ runner.os }}-cargo-${{ hashFiles('\*\*/Cargo.lock') }}

&#x20;     - run: cargo build --workspace --release



&#x20; qemu-smoke:

&#x20;   runs-on: ubuntu-latest

&#x20;   needs: build

&#x20;   steps:

&#x20;     - uses: actions/checkout@v4

&#x20;     - run: scripts/qemu-smoke.sh

```



\### scripts/qemu-smoke.sh

```bash

\#!/usr/bin/env bash

set -euo pipefail

KERNEL=target/x86\_64-unknown-none/release/turnix-kernel

TIMEOUT=60



if \[ ! -f "$KERNEL" ]; then

&#x20; echo "Kernel not found at $KERNEL"

&#x20; exit 1

fi



timeout ${TIMEOUT}s qemu-system-x86\_64 -machine accel=kvm -m 512M -nographic \\

&#x20; -kernel "$KERNEL" -serial mon:stdio -no-reboot -append "console=ttyS0 test\_mode=smoke" \\

&#x20; | tee qemu-serial.log



if grep -q "TURNIX\_OK" qemu-serial.log; then

&#x20; echo "Smoke test passed"

&#x20; exit 0

else

&#x20; echo "Smoke test failed"

&#x20; tail -n 200 qemu-serial.log

&#x20; exit 2

fi

```



\---



\## 7 Milestones, KPIs, and Acceptance Criteria



\*\*Quarterly milestones\*\*

\- \*\*Q1\*\* Kernel foundations and memory management: demand paging merged; memstress harness passing.

\- \*\*Q2\*\* Driver framework and core device support: `drivers/` model merged; virtio-net and block tests passing.

\- \*\*Q3\*\* Userspace and VFS: `init` + `shell` functional; LTP core subset passing.

\- \*\*Q4\*\* Security and packaging: MAC framework skeleton and reproducible package pipeline.



\*\*KPIs\*\*

\- CI green rate ≥ 95% on `turnix-next`.

\- Boot to `init` in < 5s on CI QEMU image.

\- Coverage ≥ 70% for `kernel/` and `mm/` within 6 months.

\- Average PR review time < 72 hours.



\*\*Acceptance tests\*\*

\- QEMU smoke test prints `TURNIX\_OK` and exits 0.

\- Driver unit tests simulate device probe/init and pass.

\- Memstress harness runs 24 hours without kernel panic.



\---



\## 8 Governance, Releases, and Community



\*\*Governance\*\*

\- `CODE\_OF\_CONDUCT.md` required.

\- `CODEOWNERS` to route kernel and mm reviews.

\- Monthly contributor sync and public meeting notes.



\*\*Releases\*\*

\- Semantic versioning.

\- GitHub Releases with signed artifacts.

\- Automated changelog generation using `git-cliff` or `release-drafter`.



\*\*Onboarding\*\*

\- `starter/` folder with 8 small tasks and tests.

\- Labeling: `good first issue`, `help wanted`, `priority/P0`.



\---



\## 9 Risk Matrix and Mitigations



| \*\*Risk\*\* | \*\*Impact\*\* | \*\*Mitigation\*\* |

|---|---:|---|

| CI cost growth | High | Start minimal, use self-hosted runners for heavy jobs |

| API churn | Medium | Freeze public kernel ABI early; deprecation windows |

| Hardware variance | Medium | Maintain small HIL lab and compatibility matrix |



\---



\## 10 Next Steps and Ready PRs I Can Produce



\*\*Immediate PRs to open\*\*

1\. `.github/workflows/ci.yml` + `rust-toolchain.toml`.

2\. `scripts/qemu-smoke.sh` + `xtask` test harness.

3\. `PULL\_REQUEST\_TEMPLATE.md` and `CONTRIBUTING.md` updates.

4\. `drivers/` skeleton with `DeviceDriver` trait and tests.

5\. `security/SECURITY.md` and `security/` skeleton.



