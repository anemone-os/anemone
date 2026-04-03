use core::fmt::{Arguments, Write};

use spin::Mutex;

use crate::fs::{write_all, STDERR_FILENO, STDOUT_FILENO};

struct Console {
    fd: usize,
}

impl Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        write_all(self.fd, s.as_bytes()).map_err(|_| core::fmt::Error)?;
        Ok(())
    }
}

static STDOUT: Mutex<Console> = Mutex::new(Console { fd: STDOUT_FILENO });
static STDERR: Mutex<Console> = Mutex::new(Console { fd: STDERR_FILENO });

pub fn __print(args: Arguments) {
    STDOUT
        .lock()
        .write_fmt(args)
        .expect("failed to print to user console");
}

pub fn __eprint(args: Arguments) {
    STDERR
        .lock()
        .write_fmt(args)
        .expect("failed to print to user stderr");
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::console::__print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {
        $crate::console::__print(format_args!("{}\n", format_args!($($arg)*)))
    };
}

#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => {
        $crate::console::__eprint(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! eprintln {
    () => {
        $crate::eprint!("\n")
    };
    ($($arg:tt)*) => {
        $crate::console::__eprint(format_args!("{}\n", format_args!($($arg)*)))
    };
}
