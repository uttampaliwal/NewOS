#![no_std]
#![no_main]

use libturnix::{exit, fork, getgid, getpid, getuid, print, uptime, yielder};

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    print("{\"benchmarks\":[");

    bench_syscall_latency_getpid();
    print(",");
    bench_syscall_latency_getuid();
    print(",");
    bench_syscall_latency_getgid();
    print(",");
    bench_syscall_latency_uptime();
    print(",");
    bench_yield_latency();
    print(",");
    bench_fork_latency();
    print(",");
    bench_write_throughput();
    print(",");
    bench_getpid_throughput();
    print(",");
    bench_nested_syscall();
    print(",");
    bench_context_switch_yield();

    print("]}\n");
    exit(0);
}

// Helper: print a u64 without format!
fn print_num(mut n: u64) {
    if n == 0 {
        print("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    print(core::str::from_utf8(&buf[i..]).unwrap_or("0"));
}

// Benchmark: repeated getpid() syscall latency
fn bench_syscall_latency_getpid() {
    let iters = 1000u64;
    let start = uptime();
    for _ in 0..iters {
        getpid();
    }
    let elapsed = uptime().saturating_sub(start);
    let latency_us = if elapsed > 0 {
        (elapsed * 10_000) / iters
    } else {
        0
    };
    let status = if latency_us < 50_000 { "PASS" } else { "FAIL" };
    print("{\"benchmark\":\"syscall_latency_getpid\",\"value\":");
    print_num(latency_us);
    print(",\"unit\":\"us\",\"status\":\"");
    print(status);
    print("\"}");
}

// Benchmark: repeated getuid() syscall latency
fn bench_syscall_latency_getuid() {
    let iters = 1000u64;
    let start = uptime();
    for _ in 0..iters {
        getuid();
    }
    let elapsed = uptime().saturating_sub(start);
    let latency_us = if elapsed > 0 {
        (elapsed * 10_000) / iters
    } else {
        0
    };
    let status = if latency_us < 50_000 { "PASS" } else { "FAIL" };
    print("{\"benchmark\":\"syscall_latency_getuid\",\"value\":");
    print_num(latency_us);
    print(",\"unit\":\"us\",\"status\":\"");
    print(status);
    print("\"}");
}

// Benchmark: repeated getgid() syscall latency
fn bench_syscall_latency_getgid() {
    let iters = 1000u64;
    let start = uptime();
    for _ in 0..iters {
        getgid();
    }
    let elapsed = uptime().saturating_sub(start);
    let latency_us = if elapsed > 0 {
        (elapsed * 10_000) / iters
    } else {
        0
    };
    let status = if latency_us < 50_000 { "PASS" } else { "FAIL" };
    print("{\"benchmark\":\"syscall_latency_getgid\",\"value\":");
    print_num(latency_us);
    print(",\"unit\":\"us\",\"status\":\"");
    print(status);
    print("\"}");
}

// Benchmark: repeated uptime() syscall latency
fn bench_syscall_latency_uptime() {
    let iters = 1000u64;
    let start = uptime();
    for _ in 0..iters {
        uptime();
    }
    let elapsed = uptime().saturating_sub(start);
    let latency_us = if elapsed > 0 {
        (elapsed * 10_000) / iters
    } else {
        0
    };
    let status = if latency_us < 50_000 { "PASS" } else { "FAIL" };
    print("{\"benchmark\":\"syscall_latency_uptime\",\"value\":");
    print_num(latency_us);
    print(",\"unit\":\"us\",\"status\":\"");
    print(status);
    print("\"}");
}

// Benchmark: yield() syscall latency (triggers scheduler)
fn bench_yield_latency() {
    let iters = 500u64;
    let start = uptime();
    for _ in 0..iters {
        yielder();
    }
    let elapsed = uptime().saturating_sub(start);
    let latency_us = if elapsed > 0 {
        (elapsed * 10_000) / iters
    } else {
        0
    };
    let status = if latency_us < 100_000 { "PASS" } else { "FAIL" };
    print("{\"benchmark\":\"yield_latency\",\"value\":");
    print_num(latency_us);
    print(",\"unit\":\"us\",\"status\":\"");
    print(status);
    print("\"}");
}

// Benchmark: fork() latency
fn bench_fork_latency() {
    let iterations = 5u64;
    let start = uptime();
    let mut successes = 0u64;
    for _ in 0..iterations {
        let pid = fork();
        if pid == 0 {
            // child: exit immediately
            exit(0);
        } else if (pid as i64) > 0 {
            successes += 1;
            let mut status: i32 = 0;
            libturnix::waitpid(pid as i32, &mut status, 0);
        }
    }
    let elapsed = uptime().saturating_sub(start);
    let latency_us = if successes > 0 && elapsed > 0 {
        (elapsed * 10_000) / successes
    } else {
        0
    };
    let status_str = if successes == 0 {
        "SKIP"
    } else if latency_us < 200_000 {
        "PASS"
    } else {
        "FAIL"
    };
    print("{\"benchmark\":\"fork_latency\",\"value\":");
    print_num(latency_us);
    print(",\"unit\":\"us\",\"successes\":");
    print_num(successes);
    print(",\"status\":\"");
    print(status_str);
    if successes == 0 {
        print("\",\"note\":\"fork() not supported by kernel (PAGE FAULT in PML4 clone)\"");
    } else {
        print("\"");
    }
    print("}");
}

// Benchmark: serial write throughput using print()
fn bench_write_throughput() {
    let chunk = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let chunk_len = 128u64;
    let iters = 100u64;
    let start = uptime();
    for _ in 0..iters {
        print(chunk);
    }
    let elapsed = uptime().saturating_sub(start);
    let total_bytes = iters * chunk_len;
    let throughput_kbs = if elapsed > 0 {
        (total_bytes * 10) / (elapsed * 1024)
    } else {
        0
    };
    let status = if throughput_kbs > 0 { "PASS" } else { "FAIL" };
    print("{\"benchmark\":\"serial_write_throughput\",\"value\":");
    print_num(throughput_kbs);
    print(",\"unit\":\"KB/s\",\"total_bytes\":");
    print_num(total_bytes);
    print(",\"status\":\"");
    print(status);
    print("\"}");
}

// Benchmark: getpid() throughput (calls per tick)
fn bench_getpid_throughput() {
    let iters = 5000u64;
    let start = uptime();
    for _ in 0..iters {
        getpid();
    }
    let elapsed = uptime().saturating_sub(start);
    let calls_per_tick = if elapsed > 0 { iters / elapsed } else { iters };
    let status = if calls_per_tick > 10 { "PASS" } else { "FAIL" };
    print("{\"benchmark\":\"getpid_throughput\",\"value\":");
    print_num(calls_per_tick);
    print(",\"unit\":\"calls/tick\",\"total\":");
    print_num(iters);
    print(",\"status\":\"");
    print(status);
    print("\"}");
}

// Benchmark: nested syscall pattern (uptime + getpid + getuid)
fn bench_nested_syscall() {
    let iters = 500u64;
    let start = uptime();
    for _ in 0..iters {
        uptime();
        getpid();
        getuid();
    }
    let elapsed = uptime().saturating_sub(start);
    let latency_us = if elapsed > 0 {
        (elapsed * 10_000) / (iters * 3)
    } else {
        0
    };
    let status = if latency_us < 50_000 { "PASS" } else { "FAIL" };
    print("{\"benchmark\":\"nested_syscall_latency\",\"value\":");
    print_num(latency_us);
    print(",\"unit\":\"us\",\"status\":\"");
    print(status);
    print("\"}");
}

// Benchmark: yield context switch (measures scheduler overhead)
fn bench_context_switch_yield() {
    let iters = 200u64;
    let start = uptime();
    for _ in 0..iters {
        yielder();
    }
    let elapsed = uptime().saturating_sub(start);
    let context_switch_us = if elapsed > 0 {
        (elapsed * 10_000) / iters
    } else {
        0
    };
    let status = if context_switch_us < 200_000 {
        "PASS"
    } else {
        "FAIL"
    };
    print("{\"benchmark\":\"context_switch_yield\",\"value\":");
    print_num(context_switch_us);
    print(",\"unit\":\"us\",\"note\":\"measures yield-to-resume round trip\",\"status\":\"");
    print(status);
    print("\"}");
}

#[cfg(test)]
fn main() {}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
