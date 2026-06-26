#![no_std]
#![no_main]

use libturnix::{exit, ls, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut buf = [0u8; 4096];
    match ls(&mut buf) {
        Some(len) => {
            if let Ok(s) = core::str::from_utf8(&buf[..len as usize]) {
                write_str(1, s);
            }
        }
        None => {
            write_str(2, "ls: failed to read directory\n");
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
