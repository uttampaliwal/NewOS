#![no_std]
#![no_main]

use libturnix::{args_after_prog_count, arg_after_prog, close, exit, open, read, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let count = args_after_prog_count();
    if count == 0 {
        wc_fd(0, None);
    } else {
        let mut total_lines: u64 = 0;
        let mut total_words: u64 = 0;
        let mut total_bytes: u64 = 0;
        let mut had_error = false;
        let multi = count > 1;
        for i in 0..count {
            let path = arg_after_prog(i).unwrap_or("");
            match open(path) {
                Some(fd) => {
                    let (l, w, b) = wc_fd(fd, Some(path));
                    close(fd);
                    total_lines += l;
                    total_words += w;
                    total_bytes += b;
                }
                None => {
                    write_str(2, "wc: ");
                    write_str(2, path);
                    write_str(2, ": No such file or directory\n");
                    had_error = true;
                }
            }
        }
        if multi {
            write_num(total_lines);
            write_str(1, " ");
            write_num(total_words);
            write_str(1, " ");
            write_num(total_bytes);
            write_str(1, " total\n");
        }
        if had_error {
            exit(1);
        }
    }
    exit(0);
}

fn wc_fd(fd: u64, path: Option<&str>) -> (u64, u64, u64) {
    let mut lines: u64 = 0;
    let mut words: u64 = 0;
    let mut bytes: u64 = 0;
    let mut in_word = false;
    let mut prev = b'\n';
    let mut buf = [0u8; 4096];
    loop {
        match read(fd, &mut buf) {
            Some(0) => break,
            Some(n) => {
                for i in 0..n as usize {
                    let c = buf[i];
                    bytes += 1;
                    if c == b'\n' {
                        lines += 1;
                    }
                    if c == b' ' || c == b'\n' || c == b'\t' || c == b'\r' {
                        in_word = false;
                    } else if !in_word {
                        in_word = true;
                        words += 1;
                    }
                    prev = c;
                }
            }
            None => break,
        }
    }
    if prev != b'\n' && bytes > 0 {
        lines += 1;
    }
    match path {
        Some(p) => {
            write_num(lines);
            write_str(1, " ");
            write_num(words);
            write_str(1, " ");
            write_num(bytes);
            write_str(1, " ");
            write_str(1, p);
            write_str(1, "\n");
        }
        None => {}
    }
    (lines, words, bytes)
}

fn write_num(n: u64) {
    if n == 0 {
        write_str(1, "0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    let mut val = n;
    while val > 0 {
        i -= 1;
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    let s = &buf[i..];
    let mut j = 0;
    while j < s.len() {
        let mut chunk = [0u8; 32];
        let len = core::cmp::min(s.len() - j, 32);
        let mut k = 0;
        while k < len {
            chunk[k] = s[j + k];
            k += 1;
        }
        write_str(1, core::str::from_utf8(&chunk[..len]).unwrap_or("?"));
        j += len;
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
