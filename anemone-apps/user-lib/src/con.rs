use core::{
    fmt::{Arguments, Write},
    str::FromStr,
};

use alloc::ffi::CString;
use anemone_abi::syscall::syscall;
use spin::Mutex;

pub struct UserWrite;
impl Write for UserWrite {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        unsafe {
            let c_str =
                CString::from_str(s).expect("failed to convert the str value to c-type string");
            syscall(100, c_str.as_ptr() as u64, 0, 0, 0, 0, 0)
                .unwrap_or_else(|e| panic!("unable to print line: {:#x}", e));
        }
        Ok(())
    }
}
static UWRITE: Mutex<UserWrite> = Mutex::new(UserWrite);

pub fn __uprint(arg: Arguments) {
    UWRITE
        .lock()
        .write_fmt(arg)
        .unwrap_or_else(|e| panic!("unable to print line: {:?}", e));
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::con::__uprint(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {
        $crate::con::__uprint(format_args!("{}\n",format_args!($($arg)*)));
    };
}
