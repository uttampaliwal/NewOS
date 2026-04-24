use core::fmt;
pub use newos_serial::{init, SerialWriter, println};

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => { newos_serial::print!($($arg)*) };
}

pub fn print(args: fmt::Arguments<'_>) {
    newos_serial::print!("{}", args);
}