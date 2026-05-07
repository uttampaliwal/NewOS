//! Minimal std port for Turnix OS
#![no_std]

extern crate core as core;
extern crate alloc;

/// Basic io module
pub mod io {
    pub trait Write {
        fn write_str(&mut self, s: &str) -> Result<(), ()>;
    }

    pub struct Stdout;

    impl Write for Stdout {
        fn write_str(&mut self, s: &str) -> Result<(), ()> {
            for c in s.chars() {
                unsafe {
                    let writer = &mut *(0 as *mut turnix_serial::SerialWriter);
                    let _ = core::fmt::Write::write_char(writer, c);
                }
            }
            Ok(())
        }
    }

    pub fn stdout() -> Stdout {
        Stdout
    }
}

/// print! macro
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        {
            let mut stdout = crate::io::stdout();
            let _ = stdout.write_str(&alloc::fmt::format!($($arg)*));
        }
    };
}

/// println! macro  
#[macro_export]
macro_rules! println {
    () => {
        crate::print!("\n");
    };
    ($($arg:tt)*) => {
        {
            crate::print!($($arg)*);
            crate::print!("\n");
        }
    };
}

    pub struct Stdout;

    impl Write for Stdout {
        fn write_str(&mut self, s: &str) -> Result<(), ()> {
            for c in s.chars() {
                unsafe {
                    let writer = &mut *(0 as *mut turnix_serial::SerialWriter);
                    let _ = core::fmt::Write::write_char(writer, c);
                }
            }
            Ok(())
        }
    }

    pub fn stdout() -> Stdout {
        Stdout
    }
}

/// print! macro
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        {
            let mut stdout = crate::io::stdout();
            let _ = stdout.write_str(&alloc::fmt::format!($($arg)*));
        }
    };
}

/// println! macro  
#[macro_export]
macro_rules! println {
    () => {
        crate::print!("\n");
    };
    ($($arg:tt)*) => {
        {
            crate::print!($($arg)*);
            crate::print!("\n");
        }
    };
}

    pub struct Stdout;

    impl Write for Stdout {
        fn write_str(&mut self, s: &str) -> Result<(), ()> {
            for c in s.chars() {
                unsafe {
                    let writer = &mut *(0 as *mut turnix_serial::SerialWriter);
                    let _ = core::fmt::Write::write_char(writer, c);
                }
            }
            Ok(())
        }
    }

    pub fn stdout() -> Stdout {
        Stdout
    }
}

/// print! macro
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        {
            let mut stdout = crate::io::stdout();
            let _ = stdout.write_str(&alloc::fmt::format!( $($arg)* ));
        }
    };
}

/// println! macro
#[macro_export]
macro_rules! println {
    () => {
        print!("\n");
    };
    ($($arg:tt)*) => {
        {
            print!($($arg)*);
            print!("\n");
        }
    };
}
