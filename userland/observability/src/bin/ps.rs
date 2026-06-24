#![no_std]
#![no_main]

use libturnix::{exit, getpid, getuid, println, uptime};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    println("PID   UID   CMD");
    println("----  ----  ----");

    let pid = getpid();
    let uid = getuid();
    let mut line = [0u8; 64];
    let mut pos = 0;

    // Write PID (up to 5 digits)
    pos = write_u64(&mut line, pos, pid);
    while pos < 5 {
        shift_right(&mut line, pos);
        pos += 1;
    }
    pos = 5;
    line[pos] = b' ';
    pos += 1;
    line[pos] = b' ';
    pos += 1;

    // Write UID
    pos = write_u64(&mut line, pos, uid);
    while pos < 11 {
        shift_right(&mut line, pos);
        pos += 1;
    }
    pos = 11;
    line[pos] = b' ';
    pos += 1;
    line[pos] = b' ';
    pos += 1;

    // Command name
    let cmd = b"init";
    let mut i = 0;
    while i < cmd.len() && pos < 64 {
        line[pos] = cmd[i];
        pos += 1;
        i += 1;
    }

    if let Ok(s) = core::str::from_utf8(&line[..pos]) {
        println(s);
    }

    println("");
    println("Note: Full process table requires a dedicated ps syscall.");
    exit(0);
}

fn write_u64(buf: &mut [u8], mut pos: usize, mut n: u64) -> usize {
    if n == 0 {
        buf[pos] = b'0';
        return pos + 1;
    }
    let start = pos;
    while n > 0 {
        buf[pos] = b'0' + (n % 10) as u8;
        n /= 10;
        pos += 1;
    }
    // Reverse
    let mut i = start;
    let mut j = pos - 1;
    while i < j {
        let tmp = buf[i];
        buf[i] = buf[j];
        buf[j] = tmp;
        i += 1;
        j -= 1;
    }
    pos
}

fn shift_right(buf: &mut [u8], pos: usize) {
    let mut i = pos;
    while i > 0 {
        buf[i] = buf[i - 1];
        i -= 1;
    }
    buf[0] = b' ';
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
