use core::fmt::{Arguments, Write};

use alloc::ffi::CString;
use spin::Mutex;

use crate::syscalls::sys_dbg_print;

struct Console;

impl Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let c_string = CString::new(s).map_err(|_| core::fmt::Error)?;
        let ptr = c_string.as_ptr() as u64;
        let len = c_string.as_bytes().len() as u64;
        // wait. an error occurred since we can't print to console, how can we call
        // `expect` to panic with a message? TODO: refine this later.
        sys_dbg_print(ptr, len).expect("failed to print to user console");
        Ok(())
    }
}

static CONSOLE: Mutex<Console> = Mutex::new(Console);

pub fn __print(args: Arguments) {
    CONSOLE
        .lock()
        .write_fmt(args)
        .expect("failed to print to user console");
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
