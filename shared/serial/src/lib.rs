#![no_std]

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, Ordering};

pub const COM1_BASE: u16 = 0x3F8;

/// Global flag to suppress serial port I/O (used in test mode).
static SERIAL_ENABLED: AtomicBool = AtomicBool::new(true);

/// Disable hardware serial port access. Called by test harnesses.
pub fn disable_serial() {
    SERIAL_ENABLED.store(false, Ordering::SeqCst);
}

/// Re-enable hardware serial port access.
pub fn enable_serial() {
    SERIAL_ENABLED.store(true, Ordering::SeqCst);
}

pub fn init() {
    if !SERIAL_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    unsafe {
        out8(COM1_BASE + 1, 0x00);
        out8(COM1_BASE + 3, 0x80);
        out8(COM1_BASE, 0x03);
        out8(COM1_BASE + 1, 0x00);
        out8(COM1_BASE + 3, 0x03);
        out8(COM1_BASE + 2, 0xC7);
        out8(COM1_BASE + 4, 0x0B);
    }
}

pub struct SerialWriter;

impl SerialWriter {
    pub fn write_byte(&mut self, byte: u8) {
        if !SERIAL_ENABLED.load(Ordering::Relaxed) {
            return;
        }
        unsafe {
            while (in8(COM1_BASE + 5) & 0x20) == 0 {}
            out8(COM1_BASE, byte);
        }
    }
}

impl Write for SerialWriter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        for byte in value.bytes() {
            match byte {
                b'\n' => {
                    self.write_byte(b'\r');
                    self.write_byte(b'\n');
                }
                _ => self.write_byte(byte),
            }
        }
        Ok(())
    }
}

/// # Safety
/// The caller must ensure `port` is a valid I/O port and the operation is safe.
pub unsafe fn out8(port: u16, value: u8) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (port, value);
        // On non-x86 architectures, port I/O doesn't exist or is mapped differently.
        // For now, this is a no-op stub.
    }
}

/// # Safety
/// The caller must ensure `port` is a valid I/O port and the operation is safe.
pub unsafe fn in8(port: u16) -> u8 {
    #[cfg(target_arch = "x86_64")]
    {
        let value: u8;
        unsafe {
            core::arch::asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
        }
        value
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = port;
        0
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::_print(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    let mut writer = SerialWriter;
    let _ = writer.write_fmt(args);
}
