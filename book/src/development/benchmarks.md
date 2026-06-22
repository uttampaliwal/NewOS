# Benchmarks

Turnix includes both host-side algorithm benchmarks and QEMU-based system benchmarks.

## Host Benchmarks

Run on the host to measure algorithmic performance of kernel data structures:

```bash
cargo run --release -p turnix-benchmarks
```

Benchmarks cover SHA-256 throughput, SAT solver resolution time, page table manipulation,
and ring-buffer I/O. Results are reported as average nanoseconds per operation and
throughput (ops/sec).

## QEMU Benchmarks

System-level benchmarks execute inside QEMU to measure real kernel behavior:

- **Syscall latency**: Round-trip time for `getpid`, `write`, and `uptime` syscalls
- **Context switch cost**: Timer-interrupt-driven preemption and reschedule timing
- **VFS throughput**: `tmpfs` and `ext4` read/write throughput at various block sizes
- **IPC throughput**: Pipe and Unix socket bytes/sec under concurrent load

## Adding Benchmarks

Add new benchmarks to `benchmarks/src/main.rs` using the `bench()` helper function.
For QEMU-based tests, extend the xtask harness in `tools/xtask/`.
