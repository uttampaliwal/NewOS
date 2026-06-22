#![no_std]
#![no_main]

use libturnix::{dmesg, exit, println};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    println("Memory Information");
    println("==================");
    println("");

    let mut buf = [0u8; 4096];
    match dmesg(&mut buf) {
        Ok(n) => {
            let output = core::str::from_utf8(&buf[..n]).unwrap_or("");
            let mut found = false;
            for line in output.lines() {
                let lower = line.as_bytes();
                if contains_ci(lower, b"memory")
                    || contains_ci(lower, b"frame")
                    || contains_ci(lower, b"heap")
                    || contains_ci(lower, b"page")
                    || contains_ci(lower, b"alloc")
                    || contains_ci(lower, b"free")
                    || contains_ci(lower, b"oom")
                {
                    println(line);
                    found = true;
                }
            }
            if !found {
                println("No memory-related messages in kernel log.");
            }
        }
        Err(_) => {
            println("Failed to read kernel log.");
        }
    }

    println("");
    println("Note: Full memory stats require a dedicated meminfo syscall.");
    exit(0);
}

fn contains_ci(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    'outer: for i in 0..=(haystack.len() - needle.len()) {
        for j in 0..needle.len() {
            let h = haystack[i + j];
            let n = needle[j];
            let h_lower = if h >= b'A' && h <= b'Z' { h + 32 } else { h };
            let n_lower = if n >= b'A' && n <= b'Z' { n + 32 } else { n };
            if h_lower != n_lower {
                continue 'outer;
            }
        }
        return true;
    }
    false
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
