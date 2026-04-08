use anemone_abi::fs::linux::{at::AT_FDCWD, mode::*, open::*};
use spin::Mutex;

use crate::{
    io::{Read, Write},
    os::linux::fs,
    prelude::*,
};

#[derive(Debug)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    create: bool,
    append: bool,
    // permission bits are not supported for now. always set to 0o644.
}

impl OpenOptions {
    pub fn new() -> Self {
        Self {
            read: false,
            write: false,
            create: false,
            append: false,
        }
    }

    pub fn read(&mut self, read: bool) -> &mut Self {
        self.read = read;
        self
    }

    pub fn write(&mut self, write: bool) -> &mut Self {
        self.write = write;
        self
    }

    pub fn create(&mut self, create: bool) -> &mut Self {
        self.create = create;
        self
    }

    pub fn append(&mut self, append: bool) -> &mut Self {
        self.append = append;
        self
    }

    pub fn open(&self, path: &Path) -> Result<File, Errno> {
        let mut flags = 0;
        if self.read && self.write {
            flags |= O_RDWR;
        } else if self.read {
            flags |= O_RDONLY;
        } else if self.write {
            flags |= O_WRONLY;
        }

        if self.create {
            flags |= O_CREAT;
        }

        if self.append {
            flags |= O_APPEND;
        }

        let fd = fs::openat(
            AT_FDCWD as usize,
            path,
            flags,
            S_IRUSR | S_IWUSR | S_IRGRP | S_IROTH,
        )?;
        Ok(File {
            inner: Mutex::new(FileInner { fd }),
        })
    }
}

#[derive(Debug)]
pub struct File {
    inner: Mutex<FileInner>,
}

#[derive(Debug)]
struct FileInner {
    fd: usize,
}

impl File {
    pub const unsafe fn from_raw_fd(fd: usize) -> Self {
        Self {
            inner: Mutex::new(FileInner { fd }),
        }
    }

    fn tx<F, R>(&self, f: F) -> Result<R, Errno>
    where
        F: FnOnce(&mut FileInner) -> Result<R, Errno>,
    {
        let mut inner = self.inner.lock();
        f(&mut inner)
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Errno> {
        self.tx(|file| fs::read(file.fd, buf))
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Errno> {
        self.tx(|file| fs::write(file.fd, buf))
    }
}

impl core::fmt::Write for &File {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let mut buf = s.as_bytes();
        while !buf.is_empty() {
            let written = self.tx(|file| fs::write(file.fd, buf))
                .map_err(|_| core::fmt::Error)?;
            if written == 0 {
                return Err(core::fmt::Error);
            }
            buf = &buf[written..];
        }

        Ok(())
    }
}
