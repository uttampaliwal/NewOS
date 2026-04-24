use core::fmt;
pub use newos_serial::{init, SerialWriter, print, println};

// For backward compatibility within the kernel
pub fn print_fmt(args: fmt::Arguments<'_>) {
    print!("{}", args);
}
