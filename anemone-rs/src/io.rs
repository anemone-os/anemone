use core::fmt::{Arguments, Write as _};

use anemone_abi::fs::linux::{STDERR_FILENO, STDOUT_FILENO};

use crate::{fs::File, prelude::*};

pub trait Read {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Errno>;

    fn read_exact(&mut self, mut buf: &mut [u8]) -> Result<(), Errno> {
        while !buf.is_empty() {
            let read = self.read(buf)?;
            if read == 0 {
                return Err(EIO);
            }
            buf = &mut buf[read..];
        }

        Ok(())
    }
}

pub trait Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Errno>;

    fn flush(&mut self) -> Result<(), Errno> {
        Ok(())
    }

    fn write_exact(&mut self, mut buf: &[u8]) -> Result<(), Errno> {
        while !buf.is_empty() {
            let written = self.write(buf)?;
            if written == 0 {
                return Err(EIO);
            }
            buf = &buf[written..];
        }

        Ok(())
    }
}

static STDOUT: File = unsafe { File::from_raw_fd(STDOUT_FILENO) };
static STDERR: File = unsafe { File::from_raw_fd(STDERR_FILENO) };

pub fn __print(args: Arguments) {
    let mut stdout = &STDOUT;
    stdout
        .write_fmt(args)
        .expect("failed to print to user stdout");
}

pub fn __eprint(args: Arguments) {
    let mut stderr = &STDERR;
    stderr
        .write_fmt(args)
        .expect("failed to print to user stderr");
}

#[macro_export]
macro_rules! print {
	($($arg:tt)*) => {
		$crate::io::__print(format_args!($($arg)*))
	};
}

#[macro_export]
macro_rules! println {
	() => {
		$crate::print!("\n")
	};
	($($arg:tt)*) => {
		$crate::io::__print(format_args!("{}\n", format_args!($($arg)*)))
	};
}

#[macro_export]
macro_rules! eprint {
	($($arg:tt)*) => {
		$crate::io::__eprint(format_args!($($arg)*))
	};
}

#[macro_export]
macro_rules! eprintln {
	() => {
		$crate::eprint!("\n")
	};
	($($arg:tt)*) => {
		$crate::io::__eprint(format_args!("{}\n", format_args!($($arg)*)))
	};
}
