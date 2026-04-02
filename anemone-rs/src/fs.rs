use alloc::ffi::CString;
use anemone_abi::{
    errno::{EIO, EINVAL, Errno},
    fs::linux::at::AT_FDCWD,
};

use crate::{
    prelude::*,
    syscalls::{
        sys_chdir, sys_chroot, sys_close, sys_dup, sys_dup3, sys_getcwd, sys_openat, sys_read,
        sys_write,
    },
};

pub type RawFd = usize;

pub use anemone_abi::fs::linux::open;
pub use anemone_abi::fs::linux::{STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};

pub fn openat(dirfd: isize, path: impl AsRef<str>, flags: u32, mode: u32) -> Result<RawFd, Errno> {
    let path = CString::new(path.as_ref()).map_err(|_| EINVAL)?;
    sys_openat(dirfd, path.as_ptr() as u64, flags, mode)
}

pub fn open(path: impl AsRef<str>, flags: u32, mode: u32) -> Result<RawFd, Errno> {
    openat(AT_FDCWD as isize, path, flags, mode)
}

pub fn read(fd: RawFd, buf: &mut [u8]) -> Result<usize, Errno> {
    sys_read(fd, buf.as_mut_ptr() as u64, buf.len())
}

pub fn write(fd: RawFd, buf: &[u8]) -> Result<usize, Errno> {
    sys_write(fd, buf.as_ptr() as u64, buf.len())
}

pub fn write_all(fd: RawFd, mut buf: &[u8]) -> Result<(), Errno> {
    while !buf.is_empty() {
        let written = write(fd, buf)?;
        if written == 0 {
            return Err(EIO);
        }
        buf = &buf[written..];
    }

    Ok(())
}

pub fn close(fd: RawFd) -> Result<(), Errno> {
    sys_close(fd)
}

pub fn dup(fd: RawFd) -> Result<RawFd, Errno> {
    sys_dup(fd)
}

pub fn dup3(oldfd: RawFd, newfd: RawFd, flags: u32) -> Result<RawFd, Errno> {
    sys_dup3(oldfd, newfd, flags)
}

pub fn stdin() -> RawFd {
    STDIN_FILENO
}

pub fn stdout() -> RawFd {
    STDOUT_FILENO
}

pub fn stderr() -> RawFd {
    STDERR_FILENO
}

pub fn getcwd() -> Result<String, Errno> {
    let mut buf = vec![0; 4096].into_boxed_slice();
    sys_getcwd(buf.as_mut_ptr() as u64, buf.len() as u64)?;
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..len].to_vec()).map_err(|_| EINVAL)
}

pub fn chdir(path: impl AsRef<str>) -> Result<(), Errno> {
    let path = CString::new(path.as_ref()).map_err(|_| EINVAL)?;
    sys_chdir(path.as_ptr() as u64)
}

pub fn chroot(path: impl AsRef<str>) -> Result<(), Errno> {
    let path = CString::new(path.as_ref()).map_err(|_| EINVAL)?;
    sys_chroot(path.as_ptr() as u64)
}
