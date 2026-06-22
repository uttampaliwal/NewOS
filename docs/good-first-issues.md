# Good First Issues

This file documents beginner-friendly issues that are great for new contributors.

## How to Help

1. Check the issue tracker for issues labeled `good first issue`
2. Read [CONTRIBUTING.md](../CONTRIBUTING.md) for setup instructions
3. Comment on the issue you'd like to work on
4. Fork, branch, and submit a PR

---

## Recommended Starter Issues

### 1. Expand unit tests for shared/abi

**Location**: `shared/abi/`
**Labels**: `testing`, `good first issue`
**Difficulty**: Beginner

Current tests cover boot info and syscall IDs. Add tests for:
- Memory descriptor validation
- BootInfo serialization
- Syscall header defaults
- Network ABI types (NetSetAddrReq, NetQueryResp)

Run tests:
```bash
cargo test -p turnix-abi
```

### 2. Improve documentation comments

**Location**: various `kernel/src/` files
**Labels**: `documentation`, `good first issue`
**Difficulty**: Beginner

Add doc comments explaining public functions and types in:
- `kernel/src/fs/vfs.rs` - VFS trait methods
- `kernel/src/security/mod.rs` - Security subsystem
- `kernel/src/net/mod.rs` - Network stack

### 3. Add error context to syscall handlers

**Location**: `kernel/src/syscall/handler.rs`
**Labels**: `enhancement`, `good first issue`
**Difficulty**: Intermediate

Many syscall handlers return generic `Error(code)` values. Add more descriptive error variants or context.

---

## Getting Help

- Open a discussion in [GitHub Discussions](https://github.com/uttampaliwal/turnix/discussions)
- Ask in the issue comments
- Review closed PRs for examples
