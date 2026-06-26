#![no_std]
#![no_main]

use libturnix::{args_after_prog_count, arg_after_prog, close, exit, open, read, unlink, write_all, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let count = args_after_prog_count();
    if count < 2 {
        write_str(2, "mv: missing file operand\n");
        exit(1);
    }
    let src = arg_after_prog(0).unwrap_or("");
    let dest = arg_after_prog(1).unwrap_or("");

    let src_fd = match open(src) {
        Some(f) => f,
        None => {
            write_str(2, "mv: cannot stat '");
            write_str(2, src);
            write_str(2, "'\n");
            exit(1);
        }
    };

    let dest_fd = match open(dest) {
        Some(f) => f,
        None => {
            close(src_fd);
            write_str(2, "mv: cannot open '");
            write_str(2, dest);
            write_str(2, "' for writing\n");
            exit(1);
        }
    };

    let mut buf = [0u8; 4096];
    loop {
        match read(src_fd, &mut buf) {
            Some(0) => break,
            Some(n) => {
                if !write_all(dest_fd, &buf[..n as usize]) {
                    write_str(2, "mv: write error\n");
                    close(src_fd);
                    close(dest_fd);
                    exit(1);
                }
            }
            None => break,
        }
    }

    close(src_fd);
    close(dest_fd);

    if !unlink(src) {
        write_str(2, "mv: cannot remove '");
        write_str(2, src);
        write_str(2, "'\n");
        exit(1);
    }

    exit(0);
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
