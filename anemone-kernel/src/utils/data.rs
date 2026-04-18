//! Random-access exact-copy helpers.
//!
//! This abstraction is intentionally narrower than a generic reader: callers
//! ask a source to copy exactly `dest.len()` bytes from a given offset, or to
//! fail. That keeps file-system short-read semantics out of higher-level users
//! such as ELF loading and page population.

use core::{fmt::Debug, marker::PhantomData};

use crate::{fs::File, prelude::*};

pub trait DataSource {
    type TError: Debug;
    fn copy_to(&self, offset: usize, dest: &mut [u8]) -> Result<(), Self::TError>;
}

#[derive(Debug)]
pub struct FileDataSource<'a> {
    file: &'a File,
    base: usize,
}

impl<'a> FileDataSource<'a> {
    pub fn new(file: &'a File, base: usize) -> Self {
        Self { file, base }
    }
}

impl DataSource for FileDataSource<'_> {
    type TError = SysError;

    fn copy_to(&self, offset: usize, dest: &mut [u8]) -> Result<(), Self::TError> {
        let offset = self
            .base
            .checked_add(offset)
            .ok_or(SysError::InvalidArgument)?;
        self.file.seek(offset)?;

        let mut copied = 0usize;
        while copied < dest.len() {
            let bytes_read = self.file.read(&mut dest[copied..])?;
            if bytes_read == 0 {
                return Err(SysError::IO);
            }
            copied += bytes_read;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct SliceDataSource<'a> {
    slice: &'a [u8],
}

impl<'a> SliceDataSource<'a> {
    pub fn new(slice: &'a [u8]) -> Self {
        Self { slice }
    }
}

impl DataSource for SliceDataSource<'_> {
    type TError = SysError;

    fn copy_to(&self, offset: usize, dest: &mut [u8]) -> Result<(), Self::TError> {
        if offset + dest.len() > self.slice.len() {
            return Err(SysError::InvalidArgument);
        }
        dest.copy_from_slice(&self.slice[offset..offset + dest.len()]);
        Ok(())
    }
}

#[derive(Debug)]
pub struct ZeroDataSource<TErr> {
    _phantom: PhantomData<TErr>,
}

impl<TErr> ZeroDataSource<TErr> {
    pub const fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}
impl<T: Debug> DataSource for ZeroDataSource<T> {
    type TError = T;

    fn copy_to(&self, _offset: usize, dest: &mut [u8]) -> Result<(), Self::TError> {
        for i in 0..dest.len() {
            dest[i] = 0;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct ClipDataSource<S: DataSource> {
    source: S,
    clip_len: usize,
}

impl<S: DataSource> ClipDataSource<S> {
    /// Previous `clip_len` bytes will be skipped, and only the following bytes
    /// will be copied to the destination.
    pub fn clip(source: S, clip_len: usize) -> Self {
        Self { source, clip_len }
    }
}

impl<S: DataSource> DataSource for ClipDataSource<S> {
    type TError = S::TError;

    fn copy_to(&self, offset: usize, dest: &mut [u8]) -> Result<(), Self::TError> {
        self.source.copy_to(offset + self.clip_len, dest)
    }
}
