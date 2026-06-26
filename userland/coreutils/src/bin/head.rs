#![no_std]
#![no_main]

use libturnix::{args_after_prog_count, arg_after_prog, close, exit, open, read, write, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let count = args_after_prog_count();
    let mut line_count: u64 = 10;
    let mut files: [&str; 16] = [""; 16];
    let mut file_count = 0;
    let mut i = 0;
    while i < count {
        let arg = arg_after_prog(i).unwrap_or("");
        if arg == "-n" {
            i += 1;
            if i < count {
                line_count = parse_num(arg_after_prog(i).unwrap_or(""));
            }
        } else {
            if file_count < 16 {
                files[file_count] = arg;
                file_count += 1;
            }
        }
        i += 1;
    }
    if file_count == 0 {
        head_stdin(line_count);
    } else {
        let mut had_error = false;
        let multi = file_count > 1;
        for j in 0..file_count {
            if multi {
                write_str(1, "==> ");
                write_str(1, files[j]);
                write_str(1, " <==\n");
            }
            match open(files[j]) {
                Some(fd) => {
                    head_fd(fd, line_count);
                    close(fd);
                }
                None => {
                    write_str(2, "head: ");
                    write_str(2, files[j]);
                    write_str(2, ": No such file or directory\n");
                    had_error = true;
                }
            }
        }
        if had_error {
            exit(1);
        }
    }
    exit(0);
}

fn head_stdin(limit: u64) {
    head_fd(0, limit);
}

fn head_fd(fd: u64, limit: u64) {
    let mut lines_printed: u64 = 0;
    let mut buf = [0u8; 4096];
    loop {
        match read(fd, &mut buf) {
            Some(0) => break,
            Some(n) => {
                let mut start = 0;
                for i in 0..n as usize {
                    if buf[i] == b'\n' {
                        lines_printed += 1;
                        write(1, &buf[start..=i]);
                        start = i + 1;
                        if lines_printed >= limit {
                            return;
                        }
                    }
                }
                if start < n as usize {
                    write(1, &buf[start..n as usize]);
                }
            }
            None => break,
        }
    }
}

fn parse_num(s: &str) -> u64 {
    let mut n: u64 = 0;
    for c in s.as_bytes() {
        if *c >= b'0' && *c <= b'9' {
            n = n * 10 + (*c - b'0') as u64;
        } else {
            break;
        }
    }
    if n == 0 { 1 } else { n }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
