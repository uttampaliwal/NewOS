#![no_std]
#![no_main]

use libturnix::{args_after_prog_count, arg_after_prog, exit, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let count = args_after_prog_count();
    let mut show_all = false;
    let mut show_kernel = false;
    let mut has_flag = false;

    for i in 0..count {
        let arg = arg_after_prog(i).unwrap_or("");
        if arg == "-a" || arg == "--all" {
            show_all = true;
            has_flag = true;
        } else if arg == "-s" || arg == "--kernel-name" {
            show_kernel = true;
            has_flag = true;
        }
    }

    if show_all || !has_flag {
        write_str(1, "Turnix OS 0.1.0 x86_64\n");
    } else if show_kernel {
        write_str(1, "Turnix\n");
    }
    exit(0);
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
