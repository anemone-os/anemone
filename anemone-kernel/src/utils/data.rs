use core::{fmt::Debug, marker::PhantomData};

use crate::{
    fs::{File, FsError},
    prelude::MmError,
};

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
    type TError = FsError;

    fn copy_to(&self, offset: usize, dest: &mut [u8]) -> Result<(), Self::TError> {
        let offset = self.base + offset;
        self.file.seek(offset)?;
        self.file.read(dest)?;
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
    type TError = MmError;

    fn copy_to(&self, offset: usize, dest: &mut [u8]) -> Result<(), Self::TError> {
        if offset + dest.len() > self.slice.len() {
            return Err(MmError::InvalidArgument);
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

    fn copy_to(&self, offset: usize, dest: &mut [u8]) -> Result<(), Self::TError> {
        for i in 0..dest.len() {
            dest[i] = 0;
        }
        Ok(())
    }
}
