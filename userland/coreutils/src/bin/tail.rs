#![no_std]
#![no_main]

use libturnix::{args_after_prog_count, arg_after_prog, close, exit, open, read, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

const MAX_LINES: usize = 1024;
const MAX_LINE_LEN: usize = 256;

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
        tail_fd(0, line_count, None);
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
                    tail_fd(fd, line_count, Some(files[j]));
                    close(fd);
                }
                None => {
                    write_str(2, "tail: ");
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

fn tail_fd(fd: u64, n: u64, _path: Option<&str>) {
    let n = n as usize;
    let mut lines: [[u8; MAX_LINE_LEN]; MAX_LINES] = [[0; MAX_LINE_LEN]; MAX_LINES];
    let mut line_lens: [usize; MAX_LINES] = [0; MAX_LINES];
    let mut line_count: usize = 0;
    let mut line_idx: usize = 0;
    let mut col: usize = 0;
    let mut buf = [0u8; 4096];
    loop {
        match read(fd, &mut buf) {
            Some(0) => break,
            Some(nb) => {
                for i in 0..nb as usize {
                    let c = buf[i];
                    if c == b'\n' {
                        line_lens[line_idx] = col;
                        line_idx = (line_idx + 1) % MAX_LINES;
                        if line_count < MAX_LINES {
                            line_count += 1;
                        }
                        col = 0;
                    } else {
                        if col < MAX_LINE_LEN {
                            lines[line_idx][col] = c;
                            col += 1;
                        }
                    }
                }
            }
            None => break,
        }
    }
    if col > 0 {
        line_lens[line_idx] = col;
        line_idx = (line_idx + 1) % MAX_LINES;
        if line_count < MAX_LINES {
            line_count += 1;
        }
    }
    let start = if line_count > n { line_count - n } else { 0 };
    let mut idx = if line_count >= MAX_LINES {
        line_idx
    } else {
        0
    };
    for i in 0..line_count {
        if i >= start {
            let data = &lines[idx][..line_lens[idx]];
            let mut j = 0;
            while j < data.len() {
                let mut chunk = [0u8; 256];
                let len = core::cmp::min(data.len() - j, 256);
                let mut k = 0;
                while k < len {
                    chunk[k] = data[j + k];
                    k += 1;
                }
                write_str(1, core::str::from_utf8(&chunk[..len]).unwrap_or("?"));
                j += len;
            }
            write_str(1, "\n");
        }
        idx = (idx + 1) % MAX_LINES;
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
