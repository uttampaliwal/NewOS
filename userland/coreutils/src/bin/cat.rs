#![no_std]
#![no_main]

use libturnix::{args_after_prog_count, arg_after_prog, close, exit, open, read, write, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let count = args_after_prog_count();
    if count == 0 {
        cat_stdin();
    } else {
        let mut had_error = false;
        for i in 0..count {
            let path = arg_after_prog(i).unwrap_or("");
            if !cat_file(path) {
                had_error = true;
            }
        }
        if had_error {
            exit(1);
        }
    }
    exit(0);
}

fn cat_stdin() {
    let mut buf = [0u8; 4096];
    loop {
        match read(0, &mut buf) {
            Some(0) => break,
            Some(n) => {
                write(1, &buf[..n as usize]);
            }
            None => break,
        }
    }
}

fn cat_file(path: &str) -> bool {
    let fd = match open(path) {
        Some(f) => f,
        None => {
            write_str(2, "cat: ");
            write_str(2, path);
            write_str(2, ": No such file or directory\n");
            return false;
        }
    };
    let mut buf = [0u8; 4096];
    loop {
        match read(fd, &mut buf) {
            Some(0) => break,
            Some(n) => {
                write(1, &buf[..n as usize]);
            }
            None => break,
        }
    }
    close(fd);
    true
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
