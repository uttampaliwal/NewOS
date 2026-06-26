#![no_std]
#![no_main]

use libturnix::{args_after_prog_count, arg_after_prog, close, exit, open, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let count = args_after_prog_count();
    if count == 0 {
        write_str(2, "touch: missing file operand\n");
        exit(1);
    }
    let mut had_error = false;
    for i in 0..count {
        let path = arg_after_prog(i).unwrap_or("");
        match open(path) {
            Some(fd) => {
                close(fd);
            }
            None => {
                write_str(2, "touch: cannot touch '");
                write_str(2, path);
                write_str(2, "'\n");
                had_error = true;
            }
        }
    }
    if had_error {
        exit(1);
    }
    exit(0);
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
