use core::{
    arch::naked_asm,
    ffi::{CStr, c_char},
    sync::atomic::{AtomicUsize, Ordering},
};

use anemone_abi::process::linux::aux_vec::{AuxvEntry, *};

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

static ARGC: AtomicUsize = AtomicUsize::new(0);
static ARGV: AtomicUsize = AtomicUsize::new(0);
static ENVP: AtomicUsize = AtomicUsize::new(0);
static AUXV: AtomicUsize = AtomicUsize::new(0);

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    #[cfg(target_arch = "riscv64")]
    naked_asm!(
        "mv a0, sp",
        "call {start_impl}",
        "ebreak",
        start_impl = sym start_impl,
    );

    #[cfg(target_arch = "loongarch64")]
    naked_asm!(
        "move $a0, $sp",
        "call {start_impl}",
        "break 0",
        start_impl = sym start_impl,
    );
}

unsafe extern "C" fn start_impl(stack_top: *const u64) -> ! {
    let argc = unsafe { *stack_top } as usize;
    let argv_ptr = unsafe { stack_top.add(1) };
    let envp_ptr = unsafe { argv_ptr.add(argc + 1) };
    let auxv_ptr = {
        let mut ptr = envp_ptr;
        unsafe {
            while *ptr != 0 {
                ptr = ptr.add(1);
            }
            ptr.add(1)
        }
    };

    ARGC.store(argc, Ordering::Release);
    ARGV.store(argv_ptr as usize, Ordering::Release);
    ENVP.store(envp_ptr as usize, Ordering::Release);
    AUXV.store(auxv_ptr as usize, Ordering::Release);

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
        let argv_ptr = ARGV.load(Ordering::Acquire) as *const u64;
        let arg_ptr = unsafe { *argv_ptr.add(self.current) } as *const u8;

        if arg_ptr.is_null() {
            None
        } else {
            let c_str = unsafe { CStr::from_ptr(arg_ptr as *const c_char) };
            self.current += 1;
            Some(c_str.to_str().expect("failed to decode process arguments"))
        }
    }
}

pub fn args() -> Args {
    Args { current: 0 }
}

pub struct Envs {
    current: usize,
}

impl Iterator for Envs {
    type Item = (&'static str, &'static str);

    fn next(&mut self) -> Option<Self::Item> {
        let envp_ptr = ENVP.load(Ordering::Acquire) as *const u64;
        let env_ptr = unsafe { *envp_ptr.add(self.current) } as *const u8;

        if env_ptr.is_null() {
            None
        } else {
            let c_str = unsafe { CStr::from_ptr(env_ptr as *const c_char) };
            self.current += 1;
            let env_str = c_str
                .to_str()
                .expect("failed to decode environment variable");
            let mut parts = env_str.splitn(2, '=');
            let key = parts.next().unwrap_or("");
            let value = parts.next().unwrap_or("");
            Some((key, value))
        }
    }
}

pub fn envs() -> Envs {
    Envs { current: 0 }
}

pub struct AuxV {
    current: usize,
}

impl Iterator for AuxV {
    type Item = AuxvEntry;

    fn next(&mut self) -> Option<Self::Item> {
        let auxv_ptr = AUXV.load(Ordering::Acquire) as *const AuxvEntry;
        let entry = unsafe { *auxv_ptr.add(self.current) };

        if entry.ty == 0 {
            None
        } else {
            self.current += 1;
            Some(entry)
        }
    }
}

/// Really really thin wrapper. we should design a better and more ergonomic API
/// later.
pub fn auxv() -> AuxV {
    AuxV { current: 0 }
}

mod auxv_helpers {
    use super::*;

    pub fn page_sz() -> Option<usize> {
        auxv().find_map(|entry| {
            if entry.ty == AT_PAGESZ {
                Some(entry.val as usize)
            } else {
                None
            }
        })
    }

    pub fn random_bytes() -> Option<[u8; 16]> {
        auxv().find_map(|entry| {
            if entry.ty == AT_RANDOM {
                let ptr = entry.val as *const u8;
                let mut buf = [0u8; 16];
                unsafe {
                    for i in 0..16 {
                        buf[i] = *ptr.add(i);
                    }
                }
                Some(buf)
            } else {
                None
            }
        })
    }

    pub fn clktck() -> Option<usize> {
        auxv().find_map(|entry| {
            if entry.ty == AT_CLKTCK {
                Some(entry.val as usize)
            } else {
                None
            }
        })
    }

    pub fn exec_fn() -> Option<&'static str> {
        auxv().find_map(|entry| {
            if entry.ty == AT_EXECFN {
                let ptr = entry.val as *const c_char;
                let c_str = unsafe { CStr::from_ptr(ptr) };
                Some(c_str.to_str().expect("failed to decode exec filename"))
            } else {
                None
            }
        })
    }

    pub fn platform() -> Option<&'static str> {
        auxv().find_map(|entry| {
            if entry.ty == AT_PLATFORM {
                let ptr = entry.val as *const c_char;
                let c_str = unsafe { CStr::from_ptr(ptr) };
                Some(c_str.to_str().expect("failed to decode platform string"))
            } else {
                None
            }
        })
    }

    pub fn base_platform() -> Option<&'static str> {
        auxv().find_map(|entry| {
            if entry.ty == AT_BASE_PLATFORM {
                let ptr = entry.val as *const c_char;
                let c_str = unsafe { CStr::from_ptr(ptr) };
                Some(
                    c_str
                        .to_str()
                        .expect("failed to decode base platform string"),
                )
            } else {
                None
            }
        })
    }
}
pub use auxv_helpers::*;
