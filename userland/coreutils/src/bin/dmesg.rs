#![no_std]
#![no_main]

use libturnix::{dmesg, exit, write, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut buf = [0u8; 4096];
    match dmesg(&mut buf) {
        Ok(n) => {
            write(1, &buf[..n]);
        }
        Err(_e) => {
            write_str(2, "dmesg: failed to read kernel messages\n");
            exit(1);
        }
    }
    exit(0);
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
