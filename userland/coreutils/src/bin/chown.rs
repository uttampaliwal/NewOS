#![no_std]
#![no_main]

use libturnix::{args_after_prog_count, arg_after_prog, exit, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let count = args_after_prog_count();
    if count < 2 {
        write_str(2, "chown: missing operand\n");
        write_str(2, "Usage: chown OWNER FILE\n");
        exit(1);
    }
    let _owner = arg_after_prog(0).unwrap_or("");
    let file = arg_after_prog(1).unwrap_or("");
    write_str(2, "chown: '");
    write_str(2, file);
    write_str(2, "' - operation not supported (stub)\n");
    exit(0);
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
