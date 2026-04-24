#![no_std]

use core::arch::asm;
use core::fmt::{self, Write};

pub const COM1_BASE: u16 = 0x3F8;

pub fn init() {
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

pub unsafe fn out8(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
}

pub unsafe fn in8(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
    }
    value
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
