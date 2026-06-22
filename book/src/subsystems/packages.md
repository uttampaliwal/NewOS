# Package Management

Turnix includes a package manager (`userland/package-manager/`) with a custom package
format and dependency resolution.

## tpkg Format

Packages use the `.tpkg` format defined in `shared/tpkg-format/`. A manifest includes the
package name, semver version, file list, and install/uninstall scripts. The format is
validated with strict rules: non-empty names, valid semver, no duplicate files, and
well-known script names only.

## SAT Solver

Dependency resolution uses a CDCL (Conflict-Driven Clause Learning) SAT solver. The
solver determines a satisfying set of package versions that avoids conflicts and respects
version constraints.

## TUF Verification

Package repositories follow The Update Framework (TUF) for supply-chain security. Packages
are verified against signed metadata before installation to prevent tampering and
replay attacks.

## Staging & Rollback

The package manager supports atomic staging of file changes and a rollback pipeline that
can revert a failed or partially-applied update to restore a known-good system state.
