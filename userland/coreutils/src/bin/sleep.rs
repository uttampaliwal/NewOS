#![no_std]
#![no_main]

use libturnix::{args_after_prog_count, arg_after_prog, exit, write_str, yielder};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let count = args_after_prog_count();
    if count == 0 {
        write_str(2, "sleep: missing operand\n");
        exit(1);
    }

    let seconds = match parse_u64(arg_after_prog(0).unwrap_or("")) {
        Some(s) => s,
        None => {
            write_str(2, "sleep: invalid time interval: ");
            write_str(2, arg_after_prog(0).unwrap_or(""));
            write_str(2, "\n");
            exit(1);
        }
    };

    let ticks = seconds * 100;
    let mut elapsed = 0u64;
    while elapsed < ticks {
        yielder();
        elapsed += 1;
    }
    exit(0);
}

fn parse_u64(s: &str) -> Option<u64> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut n: u64 = 0;
    for &b in bytes {
        if b < b'0' || b > b'9' {
            return None;
        }
        n = n.checked_mul(10)?.checked_add((b - b'0') as u64)?;
    }
    Some(n)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
