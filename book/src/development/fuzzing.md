# Fuzzing

Turnix uses `cargo-fuzz` (libFuzzer) for continuous fuzz testing of critical kernel
components.

## Fuzz Targets

All fuzz targets live in `fuzz/fuzz_targets/`:

- **fuzz_elf_parser**: Feeds malformed ELF binaries to the kernel's ELF loader to
  catch panics and out-of-bounds reads.
- **fuzz_seccomp_bpf**: Generates random BPF instruction sequences to stress the
  seccomp filter interpreter.
- **fuzz_ipc_message**: Mutates IPC protocol messages to find deserialization bugs.
- **fuzz_vfs_path**: Fuzzes VFS path resolution with malformed and deeply nested paths.
- **fuzz_syscall_args**: Exercises syscall argument validation with adversarial inputs.

## Running Fuzz Targets

```bash
cargo +nightly fuzz run fuzz_elf_parser
cargo +nightly fuzz run fuzz_seccomp_bpf
```

Each target is configured with a timeout and memory limit to catch hangs and leaks.
Corpus seeds are stored in `fuzz/corpus/`.
