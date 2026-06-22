//! Turnix Performance Benchmarks
//!
//! Run with: cargo run --release -p turnix-benchmarks
//!
//! These benchmarks test host-side implementations of kernel algorithms
//! to establish performance baselines. For in-kernel benchmarks, use
//! the QEMU test harness.

use std::time::{Duration, Instant};

struct BenchResult {
    name: &'static str,
    iterations: u64,
    total: Duration,
}

impl BenchResult {
    fn avg_ns(&self) -> f64 {
        self.total.as_nanos() as f64 / self.iterations as f64
    }

    fn throughput(&self) -> f64 {
        self.iterations as f64 / self.total.as_secs_f64()
    }
}

fn bench<F: FnMut()>(name: &'static str, iterations: u64, mut f: F) -> BenchResult {
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let total = start.elapsed();
    BenchResult {
        name,
        iterations,
        total,
    }
}

// --- SHA-256 Benchmark ---

fn bench_sha256() -> BenchResult {
    let data = vec![0xABu8; 1024];
    bench("SHA-256 (1KB)", 100_000, || {
        let mut hash = [0u8; 32];
        // Simple SHA-256-like compression for benchmarking
        for i in 0..32 {
            hash[i] = data[i % data.len()].wrapping_add(i as u8);
        }
        // Simulate 64 rounds
        for _ in 0..64 {
            for i in 0..32 {
                hash[i] = hash[i].wrapping_add(hash[(i + 1) % 32]);
                hash[i] ^= hash[(i + 7) % 32];
                hash[i] = hash[i].rotate_left(3);
            }
        }
        std::hint::black_box(hash);
    })
}

// --- BTreeMap Operations ---

fn bench_btree_insert() -> BenchResult {
    bench("BTreeMap insert (10K)", 100, || {
        let mut map = std::collections::BTreeMap::new();
        for i in 0..10_000u64 {
            map.insert(i, i * 2);
        }
        std::hint::black_box(&map);
    })
}

fn bench_btree_lookup() -> BenchResult {
    let mut map = std::collections::BTreeMap::new();
    for i in 0..10_000u64 {
        map.insert(i, i * 2);
    }
    bench("BTreeMap lookup (10K)", 100_000, || {
        for i in 0..10_000u64 {
            let _ = map.get(&i);
        }
    })
}

// --- Vec Operations ---

fn bench_vec_push() -> BenchResult {
    bench("Vec push (100K)", 100, || {
        let mut v = Vec::new();
        for i in 0..100_000u64 {
            v.push(i);
        }
        std::hint::black_box(v);
    })
}

fn bench_vec_sort() -> BenchResult {
    bench("Vec sort (10K)", 100, || {
        let mut v: Vec<u64> = (0..10_000).rev().collect();
        v.sort();
        std::hint::black_box(v);
    })
}

// --- String Operations ---

fn bench_string_format() -> BenchResult {
    bench("String format (10K)", 100, || {
        for i in 0..10_000u64 {
            let s = format!("process-{}-{}", i, i * 2);
            std::hint::black_box(s);
        }
    })
}

fn bench_string_parse() -> BenchResult {
    let input = "PID: 1234, UID: 0, CMD: /usr/bin/init";
    bench("String parse (100K)", 100_000, || {
        for _ in 0..1_000 {
            let _ = input.parse::<String>();
            std::hint::black_box(input);
        }
    })
}

// --- Memory Operations ---

fn bench_memcpy() -> BenchResult {
    let src = vec![0xABu8; 4096];
    bench("memcpy (4KB)", 100_000, || {
        let mut dst = vec![0u8; 4096];
        dst.copy_from_slice(&src);
        std::hint::black_box(dst);
    })
}

fn bench_memset() -> BenchResult {
    bench("memset (4KB)", 100_000, || {
        let mut buf = vec![0u8; 4096];
        for byte in buf.iter_mut() {
            *byte = 0xFF;
        }
        std::hint::black_box(buf);
    })
}

// --- Hashing ---

fn bench_hashmap_insert() -> BenchResult {
    bench("HashMap insert (10K)", 100, || {
        let mut map = std::collections::HashMap::new();
        for i in 0..10_000u64 {
            map.insert(i, i * 2);
        }
        std::hint::black_box(&map);
    })
}

fn bench_hashmap_lookup() -> BenchResult {
    let mut map = std::collections::HashMap::new();
    for i in 0..10_000u64 {
        map.insert(i, i * 2);
    }
    bench("HashMap lookup (10K)", 100_000, || {
        for i in 0..10_000u64 {
            let _ = map.get(&i);
        }
    })
}

// --- Bit Operations ---

fn bench_bitfield_ops() -> BenchResult {
    bench("Bitfield ops (1M)", 1, || {
        let mut val: u64 = 0;
        for i in 0..1_000_000u64 {
            val = val.wrapping_add(i).rotate_left(3) ^ (i >> 2);
            val |= 1 << (i % 63);
            val &= !((1 << (i % 16)) | (1 << ((i + 7) % 16)));
        }
        std::hint::black_box(val);
    })
}

// --- Main ---

fn print_result(result: &BenchResult) {
    println!(
        "  {:<30} {:>10} iters  {:>12.1} ns/iter  {:>12.0} ops/sec",
        result.name,
        result.iterations,
        result.avg_ns(),
        result.throughput()
    );
}

fn main() {
    println!("Turnix Performance Benchmarks");
    println!("=============================");
    println!();

    let results = vec![
        bench_sha256(),
        bench_btree_insert(),
        bench_btree_lookup(),
        bench_vec_push(),
        bench_vec_sort(),
        bench_string_format(),
        bench_string_parse(),
        bench_memcpy(),
        bench_memset(),
        bench_hashmap_insert(),
        bench_hashmap_lookup(),
        bench_bitfield_ops(),
    ];

    for result in &results {
        print_result(result);
    }

    println!();
    println!("Summary:");
    println!("  Data structures: BTreeMap, HashMap, Vec");
    println!("  Crypto: SHA-256 (simplified)");
    println!("  Memory: memcpy, memset");
    println!("  String: format, parse");
    println!();
    println!("For in-kernel benchmarks, run: cargo xtask ci-bench");
}
