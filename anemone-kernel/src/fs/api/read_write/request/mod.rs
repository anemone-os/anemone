use alloc::vec::Vec;

use anemone_abi::fs::linux::{IOV_MAX, IoVec};

use crate::{
    kconfig_defs::MAX_IOVEC_COUNT,
    prelude::{user_access::UserReadSlice, *},
    syscall::user_access::UserWriteSlice,
};

mod read;
mod write;

pub(super) use self::{read::ReadRequest, write::WriteRequest};

const MAX_RW_COUNT: usize = i32::MAX as usize & !(PagingArch::PAGE_SIZE_BYTES - 1);

static_assert!(
    MAX_IOVEC_COUNT > 0 && MAX_IOVEC_COUNT <= IOV_MAX,
    "max_iovec_count must be in 1..=IOV_MAX"
);

#[derive(Debug, Clone, Copy)]
pub(in crate::fs) struct CheckedIoVec {
    pub(in crate::fs) base: VirtAddr,
    pub(in crate::fs) len: usize,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::fs) enum IoVecDirection {
    Source,
    Destination,
}

#[derive(Debug, Clone, Copy)]
enum RequestPosition {
    Sequential,
    Positioned(usize),
}

impl RequestPosition {
    const fn offset(self) -> Option<usize> {
        match self {
            Self::Sequential => None,
            Self::Positioned(offset) => Some(offset),
        }
    }

    const fn is_sequential(self) -> bool {
        matches!(self, Self::Sequential)
    }

    fn advance(&mut self, delta: usize) -> Result<(), SysError> {
        if let Self::Positioned(offset) = self {
            *offset = offset.checked_add(delta).ok_or(SysError::InvalidArgument)?;
        }

        Ok(())
    }
}

const fn clamp_rw_count(count: usize) -> usize {
    if count > MAX_RW_COUNT {
        MAX_RW_COUNT
    } else {
        count
    }
}

pub(super) fn checked_nonnegative_offset(offset: i64) -> Result<usize, SysError> {
    if offset < 0 {
        Err(SysError::InvalidArgument)
    } else {
        Ok(offset as usize)
    }
}

pub(super) fn checked_hilo_offset(low: usize, high: usize) -> Result<usize, SysError> {
    checked_nonnegative_offset(hilo_offset(low, high))
}

pub(super) fn checked_hilo_offset_or_current(
    low: usize,
    high: usize,
) -> Result<Option<usize>, SysError> {
    match hilo_offset(low, high) {
        -1 => Ok(None),
        offset => checked_nonnegative_offset(offset).map(Some),
    }
}

fn hilo_offset(low: usize, high: usize) -> i64 {
    const HALF_LONG_BITS: u32 = usize::BITS / 2;

    ((((high as u64) << HALF_LONG_BITS) << HALF_LONG_BITS) | low as u64) as i64
}

pub(super) fn load_iovecs(
    uspace: &UserSpaceHandle,
    iov: VirtAddr,
    iovcnt: usize,
) -> Result<Vec<CheckedIoVec>, SysError> {
    let raw_iovecs = load_raw_iovecs(uspace, iov, iovcnt, SysError::InvalidArgument)?;
    checked_iovecs(raw_iovecs)
}

/// Imports a Socket message vector through the same configured count owner as
/// ordinary vector I/O while preserving Linux's message-specific count and
/// `MAX_RW_COUNT` clipping rules.
pub(in crate::fs) fn load_message_iovecs(
    uspace: &UserSpaceHandle,
    iov: VirtAddr,
    iovcnt: usize,
    direction: IoVecDirection,
) -> Result<Vec<CheckedIoVec>, SysError> {
    let raw_iovecs = load_raw_iovecs(uspace, iov, iovcnt, SysError::MessageTooLong)?;
    import_message_iovecs(uspace, raw_iovecs, direction)
}

fn load_raw_iovecs(
    uspace: &UserSpaceHandle,
    iov: VirtAddr,
    iovcnt: usize,
    count_error: SysError,
) -> Result<Vec<IoVec>, SysError> {
    if iovcnt == 0 {
        return Ok(Vec::new());
    }
    if iovcnt > MAX_IOVEC_COUNT {
        return Err(count_error);
    }

    let mut iovecs = vec![
        IoVec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        iovcnt
    ];
    {
        let mut guard = uspace.lock();
        let mut ptr_slice = UserReadSlice::try_new(iov, iovcnt, &mut guard)?;
        ptr_slice.copy_to_slice(&mut iovecs)?;
    }

    Ok(iovecs)
}

fn checked_iovecs(raw_iovecs: Vec<IoVec>) -> Result<Vec<CheckedIoVec>, SysError> {
    let mut iovecs = Vec::new();
    iovecs
        .try_reserve_exact(raw_iovecs.len())
        .map_err(|_| SysError::OutOfMemory)?;
    let mut total = 0usize;

    for raw_iovec in raw_iovecs {
        let len = usize::try_from(raw_iovec.iov_len).map_err(|_| SysError::InvalidArgument)?;
        let new_total = total.checked_add(len).ok_or(SysError::InvalidArgument)?;
        if new_total > MAX_RW_COUNT {
            return Err(SysError::InvalidArgument);
        }

        let base = VirtAddr::new(raw_iovec.iov_base as u64);

        iovecs.push(CheckedIoVec { base, len });
        total = new_total;
    }

    Ok(iovecs)
}

fn import_message_iovecs(
    uspace: &UserSpaceHandle,
    raw_iovecs: Vec<IoVec>,
    direction: IoVecDirection,
) -> Result<Vec<CheckedIoVec>, SysError> {
    let single = raw_iovecs.len() == 1;
    let mut iovecs = Vec::new();
    iovecs
        .try_reserve_exact(raw_iovecs.len())
        .map_err(|_| SysError::OutOfMemory)?;
    let mut total = 0usize;
    let mut guard = uspace.lock();

    for raw_iovec in raw_iovecs {
        let original_len = usize::try_from(raw_iovec.iov_len).map_err(|_| SysError::BadAddress)?;
        let base = VirtAddr::new(raw_iovec.iov_base as u64);
        let checked_len = if single {
            original_len.min(MAX_RW_COUNT)
        } else {
            original_len
        };
        match direction {
            IoVecDirection::Source => {
                let _ = UserReadSlice::<u8>::try_new(base, checked_len, &mut guard)?;
            },
            IoVecDirection::Destination => {
                let _ = UserWriteSlice::<u8>::try_new(base, checked_len, &mut guard)?;
            },
        }

        let len = original_len.min(MAX_RW_COUNT - total);
        iovecs.push(CheckedIoVec { base, len });
        total += len;
    }

    Ok(iovecs)
}

fn alloc_zeroed_buffer(len: usize) -> Result<Vec<u8>, SysError> {
    let mut buf = Vec::new();
    buf.try_reserve_exact(len)
        .map_err(|_| SysError::OutOfMemory)?;
    buf.resize(len, 0);
    Ok(buf)
}
