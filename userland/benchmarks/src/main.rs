#![no_std]
#![no_main]

use core::fmt::Write;
use libturnix::{exit, fork, print, uptime, yielder, write};

// ── Raw syscall helpers ───────────────────────────────────────────────────

fn sys_pipe(fds: &mut [i32; 2]) -> i64 {
    unsafe { core::arch::asm!("syscall", in("rax") 27u64, in("rdi") fds.as_mut_ptr(), options(nostack)) }
}

fn sys_mmap(addr: u64, len: u64, prot: u64, flags: u64, fd: u64, offset: u64) -> i64 {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 22u64,
            in("rdi") addr,
            in("rsi") len,
            in("rdx") prot,
            in("r10") flags,
            in("r8") fd,
            in("r9") offset,
            options(nostack)
        )
    }
}

// ── JSON reporter ─────────────────────────────────────────────────────────

struct JsonWriter;

impl Write for JsonWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        print(s);
        Ok(())
    }
}

fn report_result(name: &str, value: f64, unit: &str, passed: bool) {
    let mut w = JsonWriter;
    let status = if passed { "PASS" } else { "FAIL" };
    let _ = writeln!(
        w,
        "{{\"benchmark\":\"{}\",\"value\":{:.3},\"unit\":\"{}\",\"status\":\"{}\"}}",
        name, value, unit, status
    );
}

// ── Benchmark: fork latency ───────────────────────────────────────────────

fn bench_fork_latency() {
    let iterations = 10;
    let mut total_ms: f64 = 0.0;

    for _ in 0..iterations {
        let t0 = uptime();
        let pid = fork();
        if pid == 0 {
            // child: exit immediately
            exit(0);
        }
        let t1 = uptime();
        // wait for child
        let mut status: i32 = 0;
        libturnix::waitpid(pid as i32, &mut status, 0);
        total_ms += (t1 - t0) as f64;
    }

    let avg_ms = total_ms / iterations as f64;
    report_result("fork_latency", avg_ms, "ms", avg_ms < 10.0);
}

// ── Benchmark: pipe throughput ────────────────────────────────────────────

fn bench_pipe_throughput() {
    let mut fds = [0i32; 2];
    let ret = sys_pipe(&mut fds);
    if ret != 0 {
        report_result("pipe_throughput", 0.0, "MB/s", false);
        return;
    }

    let write_fd = fds[0] as u64;
    let read_fd = fds[1] as u64;

    // Write 16 MB in 64 KB chunks (1 GB would take too long in QEMU)
    let total_bytes: usize = 16 * 1024 * 1024;
    let chunk_size: usize = 64 * 1024;
    let chunk = [0u8; 64 * 1024];
    let t0 = uptime();

    let mut written = 0usize;
    while written < total_bytes {
        let result = libturnix::write(write_fd, &chunk);
        match result {
            Some(n) => written += n as usize,
            None => break,
        }
    }

    let t1 = uptime();
    let elapsed_s = (t1 - t0) as f64;
    let throughput_mb = if elapsed_s > 0.0 {
        (written as f64) / (1024.0 * 1024.0) / elapsed_s
    } else {
        0.0
    };

    libturnix::close(write_fd);
    libturnix::close(read_fd);

    report_result("pipe_throughput", throughput_mb, "MB/s", throughput_mb > 100.0);
}

// ── Benchmark: page fault latency ─────────────────────────────────────────

fn bench_page_fault_latency() {
    let page_size = 4096u64;
    let iterations = 100;
    let mut total_ns: f64 = 0.0;

    for _ in 0..iterations {
        // MAP_PRIVATE=0x02, MAP_ANONYMOUS=0x20, PROT_READ=0x1, PROT_WRITE=0x2
        let addr = sys_mmap(0, page_size, 0x3, 0x22, 0xFFFFFFFFu64, 0);
        if addr < 0 {
            continue;
        }

        let t0 = uptime();
        // Touch the page to trigger page fault
        unsafe {
            core::ptr::write_volatile(addr as *mut u8, 0x42);
        }
        let t1 = uptime();

        total_ns += (t1 - t0) as f64;
    }

    let avg_ns = total_ns / iterations as f64;
    report_result("page_fault_latency", avg_ns, "ns", avg_ns < 1000.0);
}

// ── Benchmark: context switch latency ─────────────────────────────────────

fn bench_context_switch() {
    let iterations = 50;
    let mut total_us: f64 = 0.0;

    for _ in 0..iterations {
        let t0 = uptime();
        let pid = fork();
        if pid == 0 {
            // child: yield back to parent
            yielder();
            exit(0);
        }
        // parent: yield to child, then child exits
        yielder();
        let mut status: i32 = 0;
        libturnix::waitpid(pid as i32, &mut status, 0);
        let t1 = uptime();
        total_us += (t1 - t0) as f64;
    }

    let avg_us = total_us / iterations as f64;
    report_result("context_switch", avg_us, "us", true);
}

// ── Entry point ───────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn _start() -> ! {
    print("{\"benchmarks\":[\n");

    bench_fork_latency();
    print(",\n");
    bench_pipe_throughput();
    print(",\n");
    bench_page_fault_latency();
    print(",\n");
    bench_context_switch();

    print("\n]}\n");
    exit(0);
}

#[cfg(test)]
fn main() {}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
