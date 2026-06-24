#![no_std]
#![no_main]

use libturnix::{capget, exit, getgid, getuid, println};
use turnix_abi::syscall::{CapData, CapHeader};

#[cfg(not(test))]
use core::panic::PanicInfo;

const CAP_NAMES: &[(&str, u8)] = &[
    ("CHOWN", 0),
    ("DAC_OVERRIDE", 1),
    ("DAC_READ_SEARCH", 2),
    ("FOWNER", 3),
    ("FSETID", 4),
    ("KILL", 5),
    ("SETGID", 6),
    ("SETUID", 7),
    ("SETPCAP", 8),
    ("LINUX_IMMUTABLE", 9),
    ("NET_BIND_SERVICE", 10),
    ("NET_BROADCAST", 11),
    ("NET_ADMIN", 12),
    ("NET_RAW", 13),
    ("IPC_LOCK", 14),
    ("IPC_OWNER", 15),
    ("SYS_MODULE", 16),
    ("SYS_RAWIO", 17),
    ("SYS_CHROOT", 18),
    ("SYS_PTRACE", 19),
    ("SYS_PACCT", 20),
    ("SYS_ADMIN", 21),
    ("SYS_BOOT", 22),
    ("SYS_NICE", 23),
    ("SYS_RESOURCE", 24),
    ("SYS_TIME", 25),
    ("SYS_TTY_CONFIG", 26),
    ("MKNOD", 27),
    ("LEASE", 28),
    ("AUDIT_WRITE", 29),
    ("AUDIT_CONTROL", 30),
    ("SETFCAP", 31),
];

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    println("Capability state for current process");
    println("====================================");
    println("");

    let uid = getuid();
    let gid = getgid();

    let mut line = [0u8; 32];
    let mut pos = 0;
    line[0] = b'U';
    line[1] = b'I';
    line[2] = b'D';
    line[3] = b':';
    line[4] = b' ';
    pos = 5;
    pos = write_u64(&mut line, pos, uid);
    if let Ok(s) = core::str::from_utf8(&line[..pos]) {
        println(s);
    }

    pos = 0;
    line[0] = b'G';
    line[1] = b'I';
    line[2] = b'D';
    line[3] = b':';
    line[4] = b' ';
    pos = 5;
    pos = write_u64(&mut line, pos, gid);
    if let Ok(s) = core::str::from_utf8(&line[..pos]) {
        println(s);
    }

    println("");
    println("Effective capabilities:");

    let header = CapHeader {
        version: 0x20080522,
        pid: 0,
    };
    let mut data = CapData {
        effective: [0; 2],
        permitted: [0; 2],
        inheritable: [0; 2],
    };

    let result = capget(&header, &mut data);
    if result == 0 {
        let eff = (data.effective[0] as u64) | ((data.effective[1] as u64) << 32);
        let perm = (data.permitted[0] as u64) | ((data.permitted[1] as u64) << 32);
        let inh = (data.inheritable[0] as u64) | ((data.inheritable[1] as u64) << 32);

        let mut any = false;
        for (name, bit) in CAP_NAMES {
            let has_eff = (eff >> bit) & 1 == 1;
            let has_perm = (perm >> bit) & 1 == 1;
            let has_inh = (inh >> bit) & 1 == 1;

            if has_eff || has_perm || has_inh {
                any = true;
                let mut buf = [0u8; 80];
                let mut p = 0;
                // "  NAME"
                buf[p] = b' ';
                p += 1;
                buf[p] = b' ';
                p += 1;
                for &b in name.as_bytes() {
                    if p < 80 {
                        buf[p] = b;
                        p += 1;
                    }
                }
                // " eff=Y perm=Y inh=Y"
                let eff_str = if has_eff { b" eff=Y" } else { b" eff=N" };
                let perm_str = if has_perm { b" perm=Y" } else { b" perm=N" };
                let inh_str = if has_inh { b" inh=Y" } else { b" inh=N" };
                for &b in eff_str {
                    if p < 80 {
                        buf[p] = b;
                        p += 1;
                    }
                }
                for &b in perm_str {
                    if p < 80 {
                        buf[p] = b;
                        p += 1;
                    }
                }
                for &b in inh_str {
                    if p < 80 {
                        buf[p] = b;
                        p += 1;
                    }
                }
                if let Ok(s) = core::str::from_utf8(&buf[..p]) {
                    println(s);
                }
            }
        }

        if !any {
            println("  (no capabilities set)");
        }
    } else {
        println("  Failed to get capabilities");
    }

    exit(0);
}

fn write_u64(buf: &mut [u8], mut pos: usize, mut n: u64) -> usize {
    if n == 0 {
        if pos < buf.len() {
            buf[pos] = b'0';
            return pos + 1;
        }
        return pos;
    }
    let start = pos;
    while n > 0 && pos < buf.len() {
        buf[pos] = b'0' + (n % 10) as u8;
        n /= 10;
        pos += 1;
    }
    let mut i = start;
    let mut j = pos - 1;
    while i < j {
        let tmp = buf[i];
        buf[i] = buf[j];
        buf[j] = tmp;
        i += 1;
        j -= 1;
    }
    pos
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
