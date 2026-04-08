pub mod fs {
    use alloc::ffi::CString;

    use crate::{prelude::*, sys::linux::fs};

    pub use anemone_abi::fs::linux::{STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};

    pub fn chroot(path: &str) -> Result<(), Errno> {
        let path = CString::new(path).map_err(|_| EINVAL)?;
        fs::chroot(path.as_ptr() as u64)
    }

    pub fn chdir(path: &str) -> Result<(), Errno> {
        let path = CString::new(path).map_err(|_| EINVAL)?;
        fs::chdir(path.as_ptr() as u64)
    }

    pub fn getcwd(buf: &mut [u8]) -> Result<(), Errno> {
        fs::getcwd(buf.as_mut_ptr() as u64, buf.len() as u64)
    }

    pub fn openat(dirfd: usize, path: &Path, flags: u32, mode: u32) -> Result<usize, Errno> {
        let path = CString::new(path.to_str().ok_or(EINVAL)?).map_err(|_| EINVAL)?;
        fs::openat(dirfd as isize, path.as_ptr() as u64, flags, mode)
    }

    pub fn close(fd: usize) -> Result<(), Errno> {
        fs::close(fd)
    }

    pub fn read(fd: usize, buf: &mut [u8]) -> Result<usize, Errno> {
        fs::read(fd, buf.as_mut_ptr() as u64, buf.len())
    }

    pub fn write(fd: usize, buf: &[u8]) -> Result<usize, Errno> {
        fs::write(fd, buf.as_ptr() as u64, buf.len())
    }
}

pub mod process {
    use alloc::ffi::CString;

    use crate::{prelude::*, sys::linux::process};

    pub fn execve(path: &str, argv: &[&str]) -> Result<u64, Errno> {
        let mut argv_ptrs = vec![0; argv.len() + 1].into_boxed_slice();
        let argv = argv
            .iter()
            .map(|arg| CString::new(*arg).map_err(|_| EINVAL))
            .collect::<Result<Vec<CString>, Errno>>()?;

        for (index, arg) in argv.iter().enumerate() {
            argv_ptrs[index] = arg.as_ptr() as u64;
        }
        argv_ptrs[argv.len()] = 0;

        let path = CString::new(path).map_err(|_| EINVAL)?;
        process::execve(path.as_ptr() as u64, argv_ptrs.as_ptr() as u64)
    }

    pub fn sched_yield() -> Result<(), Errno> {
        process::sched_yield()
    }

    pub fn exit(xcode: i32) -> ! {
        process::exit(xcode as u64)
    }
}
