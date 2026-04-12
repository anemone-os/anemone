use core::{
    ffi::{CStr, c_char},
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{anemone_main, os::linux, prelude::*, process::exit};

pub fn current_dir() -> Result<PathBuf, Errno> {
    let mut buf_len = 256;
    loop {
        let mut buf = vec![0; buf_len].into_boxed_slice();
        match linux::fs::getcwd(&mut buf) {
            Ok(()) => {
                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                return String::from_utf8(buf[..len].to_vec())
                    .map(PathBuf::from)
                    .map_err(|_| EINVAL);
            },
            Err(ERANGE) => {
                // Buffer is too small, double the size and try again.
                buf_len *= 2;
            },
            Err(errno) => return Err(errno),
        }
    }
}

pub fn set_current_dir(path: &Path) -> Result<(), Errno> {
    linux::fs::chdir(path.to_str().ok_or(EINVAL)?)
}

static START_ARGS_PTR: AtomicUsize = AtomicUsize::new(0);

#[unsafe(no_mangle)]
pub extern "C" fn _start(stack_top: *const u64) -> ! {
    START_ARGS_PTR.store(stack_top as usize, Ordering::Release);
    crate::allocator::init();

    match unsafe { anemone_main() } {
        Ok(()) => exit(0),
        Err(errno) => exit(errno as i8),
    }
}

pub struct Args {
    current: usize,
}

impl Iterator for Args {
    type Item = &'static str;

    fn next(&mut self) -> Option<Self::Item> {
        let start_args_ptr = START_ARGS_PTR.load(Ordering::Acquire) as *const u64;
        if start_args_ptr.is_null() {
            return None;
        }

        unsafe {
            let len = *start_args_ptr as usize;
            if self.current >= len {
                return None;
            }

            let ptr = *start_args_ptr.add(1 + self.current) as *const u8;
            let c_str = CStr::from_ptr(ptr as *const c_char);
            self.current += 1;
            Some(c_str.to_str().expect("failed to decode process arguments"))
        }
    }
}

pub fn args() -> Args {
    Args { current: 0 }
}
