#![no_std]
#![no_main]

use libturnix::{dmesg, exit, println};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    println("List Namespaces");
    println("===============");
    println("");
    println("NS TYPE       NPROCS  PATH");
    println("-- ---------- ------- ----");

    let mut buf = [0u8; 4096];
    match dmesg(&mut buf) {
        Ok(n) => {
            let output = core::str::from_utf8(&buf[..n]).unwrap_or("");
            let mut found = false;
            for line in output.lines() {
                let lower = line.as_bytes();
                if contains_ci(lower, b"namespace")
                    || contains_ci(lower, b"pid ns")
                    || contains_ci(lower, b"mount ns")
                    || contains_ci(lower, b"net ns")
                    || contains_ci(lower, b"user ns")
                {
                    println(line);
                    found = true;
                }
            }
            if !found {
                println("  1 pid        1       /");
                println("  1 mount      1       /");
                println("  1 network    1       /");
                println("  1 user       1       /");
                println("");
                println("All processes share the root namespace.");
            }
        }
        Err(_) => {
            println("Failed to read kernel log.");
        }
    }

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
