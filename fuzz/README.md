# Turnix Fuzz Testing

Fuzz testing targets for kernel parsing components. Each target reads raw
bytes from stdin and exercises a specific parser.

## Targets

| Target | Component | What It Tests |
|--------|-----------|---------------|
| `fuzz_elf_parser` | ELF header/program header parsing | Malformed ELF binaries |
| `fuzz_seccomp_bpf` | Seccomp BPF interpreter | Crafted BPF programs |
| `fuzz_ipc_message` | IPC message deserialization | Malformed IPC packets |
| `fuzz_vfs_path` | VFS path normalization | Path traversal, edge cases |
| `fuzz_syscall_args` | Syscall argument decoding | Arbitrary syscall numbers |

## Building

```bash
cd fuzz
cargo build --release
```

## Running

Each target reads from stdin:

```bash
# Fuzz ELF parser with a file
./target/release/fuzz_elf_parser < /path/to/crash.bin

# Fuzz with random data
echo "random bytes" | ./target/release/fuzz_seccomp_bpf

# Pipe from a fuzzer
cat crash_input | ./target/release/fuzz_ipc_message
```

## Integration with cargo-fuzz / libfuzzer

To use with `cargo-fuzz` (requires nightly):

```bash
# Install cargo-fuzz
cargo install cargo-fuzz

# Create fuzz target (example)
cargo fuzz init fuzz_elf_parser

# Run fuzzer
cargo fuzz run fuzz_elf_parser
```

## Integration with AFL / Honggfuzz

For coverage-guided fuzzing:

```bash
# AFL
afl-fuzz -i seeds/ -o findings/ ./target/release/fuzz_elf_parser

# Honggfuzz
honggfuzz -i seeds/ -o findings/ -- ./target/release/fuzz_elf_parser
```

## Adding a New Target

1. Create `fuzz_targets/fuzz_<name>.rs`
2. Add `[[bin]]` entry to `Cargo.toml`
3. Implement stdin reading + parser invocation
4. Test with `cargo build --release`
