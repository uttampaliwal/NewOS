#![no_std]
#![no_main]

use libturnix::{args_after_prog_count, arg_after_prog, close, exit, open, read, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let count = args_after_prog_count();
    if count == 0 {
        write_str(2, "Usage: grep pattern [file...]\n");
        exit(1);
    }
    let pattern = arg_after_prog(0).unwrap_or("");
    if count <= 1 {
        grep_fd(0, pattern, None);
    } else {
        let mut had_error = false;
        for i in 1..count {
            let path = arg_after_prog(i).unwrap_or("");
            match open(path) {
                Some(fd) => {
                    grep_fd(fd, pattern, Some(path));
                    close(fd);
                }
                None => {
                    write_str(2, "grep: ");
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

fn grep_fd(fd: u64, pattern: &str, path: Option<&str>) {
    let pattern_bytes = pattern.as_bytes();
    let mut line_buf = [0u8; 4096];
    let mut line_len: usize = 0;
    let mut buf = [0u8; 4096];
    loop {
        match read(fd, &mut buf) {
            Some(0) => {
                if line_len > 0 && contains(&line_buf[..line_len], pattern_bytes) {
                    print_line(&line_buf[..line_len], path);
                }
                break;
            }
            Some(n) => {
                let mut i = 0;
                while i < n as usize {
                    if buf[i] == b'\n' {
                        if contains(&line_buf[..line_len], pattern_bytes) {
                            print_line(&line_buf[..line_len], path);
                        }
                        line_len = 0;
                    } else {
                        if line_len < 4096 {
                            line_buf[line_len] = buf[i];
                            line_len += 1;
                        }
                    }
                    i += 1;
                }
            }
            None => break,
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    let mut i = 0;
    while i <= haystack.len() - needle.len() {
        let mut found = true;
        let mut j = 0;
        while j < needle.len() {
            if haystack[i + j] != needle[j] {
                found = false;
                break;
            }
            j += 1;
        }
        if found {
            return true;
        }
        i += 1;
    }
    false
}

fn print_line(line: &[u8], path: Option<&str>) {
    if let Some(p) = path {
        write_str(1, p);
        write_str(1, ":");
    }
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
