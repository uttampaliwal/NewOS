#![no_std]
#![no_main]

use libturnix::{args_after_prog_count, arg_after_prog, exit, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let count = args_after_prog_count();
    if count == 0 {
        write_str(1, "\n");
    } else {
        for i in 0..count {
            let arg = arg_after_prog(i).unwrap_or("");
            if i > 0 {
                write_str(1, " ");
            }
            write_str(1, arg);
        }
        write_str(1, "\n");
    }
    exit(0);
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
