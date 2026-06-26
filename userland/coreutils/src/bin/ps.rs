#![no_std]
#![no_main]

use libturnix::{exit, getpid, getuid, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    write_str(1, "PID   UID   CMD\n");
    let pid = getpid();
    let uid = getuid();
    write_u64(pid);
    write_str(1, "   ");
    write_u64(uid);
    write_str(1, "   ");
    write_str(1, "coreutils\n");
    exit(0);
}

fn write_u64(mut n: u64) {
    if n == 0 {
        write_str(1, "0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    write_str(1, core::str::from_utf8(&buf[i..]).unwrap_or("0"));
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
