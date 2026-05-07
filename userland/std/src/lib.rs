//! Minimal std port for Turnix OS
//! Provides a std-like API for userland applications.

#![no_std]

extern crate alloc;

/// IO module
pub mod io {
    use core::fmt;

    /// Write trait for output
    pub trait Write {
        fn write_str(&mut self, s: &str) -> Result<(), ()>;
        fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<(), ()> {
            let s = alloc::format!("{}", args);
            self.write_str(&s)
        }
    }

    /// Standard output
    pub struct Stdout;

    impl Write for Stdout {
        fn write_str(&mut self, s: &str) -> Result<(), ()> {
            // Use libturnix to write to stdout (fd=1)
            let fd = 1u64;
            let _ = libturnix::write(fd, s.as_bytes());
            Ok(())
        }
    }

    /// Get stdout
    pub fn stdout() -> Stdout {
        Stdout
    }
}

/// print! macro
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        {
            let mut stdout = $crate::io::stdout();
            let _ = stdout.write_fmt(core::format_args!($($arg)*));
        }
    };
}

/// println! macro
#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n");
    };
    ($($arg:tt)*) => {
        {
            $crate::print!($($arg)*);
            $crate::print!("\n");
        }
    };
}

/// Basic prelude
pub mod prelude {
    pub use crate::{print, println};
}

/// Process functions
pub mod process {
    pub fn abort() -> ! {
        libturnix::exit(1);
    }
}
