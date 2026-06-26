#![no_std]
#![no_main]

use libturnix::{args_after_prog_count, arg_after_prog, close, exit, open, read, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let count = args_after_prog_count();
    if count == 0 {
        uniq_fd(0);
    } else {
        let mut had_error = false;
        for i in 0..count {
            let path = arg_after_prog(i).unwrap_or("");
            match open(path) {
                Some(fd) => {
                    uniq_fd(fd);
                    close(fd);
                }
                None => {
                    write_str(2, "uniq: ");
                    write_str(2, path);
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

fn uniq_fd(fd: u64) {
    let mut prev_line = [0u8; 4096];
    let mut prev_len: usize = 0;
    let mut has_prev = false;
    let mut cur_line = [0u8; 4096];
    let mut cur_len: usize = 0;
    let mut buf = [0u8; 4096];
    loop {
        match read(fd, &mut buf) {
            Some(0) => {
                if cur_len > 0 {
                    write_line(&cur_line[..cur_len]);
                }
                break;
            }
            Some(n) => {
                let mut i = 0;
                while i < n as usize {
                    if buf[i] == b'\n' {
                        let same = if has_prev && cur_len == prev_len {
                            let mut j = 0;
                            let mut eq = true;
                            while j < cur_len {
                                if cur_line[j] != prev_line[j] {
                                    eq = false;
                                    break;
                                }
                                j += 1;
                            }
                            eq
                        } else {
                            false
                        };
                        if !same {
                            write_line(&cur_line[..cur_len]);
                        }
                        let mut j = 0;
                        while j < cur_len {
                            prev_line[j] = cur_line[j];
                            j += 1;
                        }
                        prev_len = cur_len;
                        has_prev = true;
                        cur_len = 0;
                        i += 1;
                    } else {
                        if cur_len < 4096 {
                            cur_line[cur_len] = buf[i];
                            cur_len += 1;
                        }
                        i += 1;
                    }
                }
            }
            None => break,
        }
    }
}

fn write_line(line: &[u8]) {
    let mut i = 0;
    while i < line.len() {
        let mut chunk = [0u8; 256];
        let len = core::cmp::min(line.len() - i, 256);
        let mut j = 0;
        while j < len {
            chunk[j] = line[i + j];
            j += 1;
        }
        write_str(1, core::str::from_utf8(&chunk[..len]).unwrap_or("?"));
        i += len;
    }
    write_str(1, "\n");
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
