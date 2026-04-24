# Good First Issues

This file documents beginner-friendly issues that are great for new contributors.

## How to Help

1. Check the issue tracker for issues labeled `good first issue`
2. Read [CONTRIBUTING.md](../CONTRIBUTING.md) for setup instructions
3. Comment on the issue you'd like to work on
4. Fork, branch, and submit a PR

---

## Recommended Starter Issues

### 1. Improve documentation comments

**Location**: Various files in `kernel/` and `boot/`
**Labels**: `documentation`, `good first issue`
**Difficulty**: Beginner

Add doc comments to public functions that lack them. Check with:

```powershell
cargo doc --document-private-items
```

### 2. Unit tests for shared/abi

**Location**: `shared/abi/`
**Labels**: `testing`, `good first issue`
**Difficulty**: Beginner

Add unit tests for the ABI types. Run:

```powershell
cargo test -p newos-abi
```

### 3. Memory allocator visualization

**Location**: `kernel/src/memory/`
**Labels**: `enhancement`, `good first issue`
**Difficulty**: Intermediate

Add serial output showing allocator state for debugging.

### 4. Interrupt handler tests

**Location**: `kernel/src/interrupts/`
**Labels**: `testing`, `good first issue`
**Difficulty**: Intermediate

Verify timer interrupts fire at expected intervals.

---

## Getting Help

- Open a discussion in [GitHub Discussions](https://github.com/uttampaliwal/NewOS/discussions)
- Ask in the issue comments
- Review closed PRs for examples