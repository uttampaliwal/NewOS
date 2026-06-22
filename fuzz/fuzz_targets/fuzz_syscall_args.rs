//! Fuzz target for syscall argument decoding.
//!
//! Reads raw bytes and validates syscall number + argument decoding.

use std::io::{self, Read};

fn decode_syscall_number(nr: u64) -> &'static str {
    match nr {
        0 => "read",
        1 => "write",
        2 => "open",
        3 => "close",
        4 => "stat",
        5 => "fstat",
        6 => "lstat",
        7 => "poll",
        8 => "lseek",
        9 => "mmap",
        10 => "mprotect",
        11 => "munmap",
        12 => "brk",
        16 => "ioctl",
        21 => "access",
        22 => "pipe",
        23 => "select",
        24 => "sched_yield",
        32 => "dup",
        33 => "dup2",
        35 => "nanosleep",
        56 => "clone",
        57 => "fork",
        58 => "vfork",
        59 => "execve",
        60 => "exit",
        61 => "wait4",
        62 => "kill",
        63 => "uname",
        72 => "fcntl",
        78 => "getdents",
        79 => "getcwd",
        80 => "chdir",
        82 => "rename",
        83 => "mkdir",
        84 => "rmdir",
        86 => "link",
        87 => "unlink",
        88 => "symlink",
        89 => "readlink",
        90 => "chmod",
        92 => "chown",
        96 => "gettimeofday",
        102 => "getpid",
        137 => "getuid",
        143 => "getgid",
        156 => "prctl",
        157 => "arch_prctl",
        160 => "setrlimit",
        186 => "gettid",
        217 => "getdents64",
        231 => "exit_group",
        257 => "openat",
        262 => "newfstatat",
        263 => "unlinkat",
        288 => "accept4",
        292 => "pread64",
        293 => "pwrite64",
        318 => "getrandom",
        322 => "execveat",
        332 => "statx",
        _ => "unknown",
    }
}

fn validate_string_arg(ptr: u64, len: u64) -> bool {
    ptr != 0
        && ptr < 0x0000_7fff_ffff_ffff
        && len <= 4096
        && ptr.checked_add(len).is_some()
}

fn main() {
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf).unwrap();

    if buf.len() < 8 {
        return;
    }

    let nr = u64::from_le_bytes(buf[0..8].try_into().unwrap());
    let _name = decode_syscall_number(nr);

    // Parse 6 args (8 bytes each = 48 bytes)
    if buf.len() >= 56 {
        let args: Vec<u64> = (0..6)
            .map(|i| u64::from_le_bytes(buf[8 + i * 8..16 + i * 8].try_into().unwrap()))
            .collect();

        // Validate string pointer args for write-like syscalls
        if nr == 1 {
            // write(fd, buf, count)
            let _ = validate_string_arg(args[1], args[2]);
        }
        // Validate path args for open-like syscalls
        if nr == 2 || nr == 257 {
            let _ = validate_string_arg(args[0], 4096);
        }
    }
}
