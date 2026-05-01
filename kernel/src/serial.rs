use core::fmt;
pub use turnix_serial::{SerialWriter, init, println};

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => { turnix_serial::print!($($arg)*) };
}

pub fn print(args: fmt::Arguments<'_>) {
    turnix_serial::print!("{}", args);
}
