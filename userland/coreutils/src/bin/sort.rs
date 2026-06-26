#![no_std]
#![no_main]

use libturnix::{args_after_prog_count, arg_after_prog, close, exit, open, read, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

const MAX_LINES: usize = 256;
const MAX_LINE_LEN: usize = 256;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let count = args_after_prog_count();
    if count == 0 {
        sort_fd(0);
    } else {
        let mut had_error = false;
        for i in 0..count {
            let path = arg_after_prog(i).unwrap_or("");
            match open(path) {
                Some(fd) => {
                    sort_fd(fd);
                    close(fd);
                }
                None => {
                    write_str(2, "sort: ");
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

fn sort_fd(fd: u64) {
    let mut lines: [[u8; MAX_LINE_LEN]; MAX_LINES] = [[0; MAX_LINE_LEN]; MAX_LINES];
    let mut line_lens: [usize; MAX_LINES] = [0; MAX_LINES];
    let mut line_count: usize = 0;
    let mut col: usize = 0;
    let mut buf = [0u8; 4096];
    loop {
        match read(fd, &mut buf) {
            Some(0) => break,
            Some(nb) => {
                for i in 0..nb as usize {
                    let c = buf[i];
                    if c == b'\n' {
                        if line_count < MAX_LINES {
                            line_lens[line_count] = col;
                            line_count += 1;
                        }
                        col = 0;
                    } else {
                        if line_count < MAX_LINES && col < MAX_LINE_LEN {
                            lines[line_count][col] = c;
                            col += 1;
                        }
                    }
                }
            }
            None => break,
        }
    }
    if col > 0 && line_count < MAX_LINES {
        line_lens[line_count] = col;
        line_count += 1;
    }
    insertion_sort(&mut lines, &mut line_lens, line_count);
    for i in 0..line_count {
        let mut j = 0;
        while j < line_lens[i] {
            let mut chunk = [0u8; 256];
            let len = core::cmp::min(line_lens[i] - j, 256);
            let mut k = 0;
            while k < len {
                chunk[k] = lines[i][j + k];
                k += 1;
            }
            write_str(1, core::str::from_utf8(&chunk[..len]).unwrap_or("?"));
            j += len;
        }
        write_str(1, "\n");
    }
}

fn insertion_sort(lines: &mut [[u8; MAX_LINE_LEN]; MAX_LINES], lens: &mut [usize; MAX_LINES], n: usize) {
    let mut i = 1;
    while i < n {
        let mut j = i;
        while j > 0 {
            if compare(&lines[j], lens[j], &lines[j - 1], lens[j - 1]) == Ordering::Less {
                let tmp_line = lines[j];
                let tmp_len = lens[j];
                lines[j] = lines[j - 1];
                lens[j] = lens[j - 1];
                lines[j - 1] = tmp_line;
                lens[j - 1] = tmp_len;
                j -= 1;
            } else {
                break;
            }
        }
        i += 1;
    }
}

fn compare(a: &[u8; MAX_LINE_LEN], a_len: usize, b: &[u8; MAX_LINE_LEN], b_len: usize) -> Ordering {
    let len = core::cmp::min(a_len, b_len);
    let mut i = 0;
    while i < len {
        if a[i] < b[i] {
            return Ordering::Less;
        }
        if a[i] > b[i] {
            return Ordering::Greater;
        }
        i += 1;
    }
    if a_len < b_len {
        Ordering::Less
    } else if a_len > b_len {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

#[derive(PartialEq)]
enum Ordering {
    Less,
    Equal,
    Greater,
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
