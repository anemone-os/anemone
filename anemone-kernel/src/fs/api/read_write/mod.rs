//! read & write system calls.

use alloc::vec::Vec;

use anemone_abi::fs::linux::IoVec;

use crate::{
    prelude::{
        user_access::{UserReadSlice, UserWriteSlice},
        *,
    },
    task::files::{Fd, FileDesc, OpenedFileReadUserSegment},
};

pub mod pread64;
pub mod preadv;
pub mod pwrite64;
pub mod pwritev;
pub mod pwritev2;
pub mod read;
pub mod readv;
pub mod write;
pub mod writev;

// TODO: make this a kconfig item.
const MAX_IOVEC_CNT: usize = 1024;
const MAX_RW_COUNT: usize = i32::MAX as usize & !(PagingArch::PAGE_SIZE_BYTES - 1);

#[derive(Debug, Clone, Copy)]
struct CheckedIoVec {
    pub base: VirtAddr,
    pub len: usize,
}

fn current_file_and_uspace(fd: Fd) -> Result<(Arc<FileDesc>, Arc<UserSpaceHandle>), SysError> {
    let task = get_current_task();
    let file = task.get_fd(fd)?;
    let uspace = task.clone_uspace_handle();

    Ok((file, uspace))
}

const fn clamp_rw_count(count: usize) -> usize {
    if count > MAX_RW_COUNT {
        MAX_RW_COUNT
    } else {
        count
    }
}

fn checked_nonnegative_offset(offset: i64) -> Result<usize, SysError> {
    if offset < 0 {
        Err(SysError::InvalidArgument)
    } else {
        Ok(offset as usize)
    }
}

fn checked_hilo_offset(low: usize, high: usize) -> Result<usize, SysError> {
    checked_nonnegative_offset(hilo_offset(low, high))
}

fn checked_hilo_offset_or_current(low: usize, high: usize) -> Result<Option<usize>, SysError> {
    match hilo_offset(low, high) {
        -1 => Ok(None),
        offset => checked_nonnegative_offset(offset).map(Some),
    }
}

fn hilo_offset(low: usize, high: usize) -> i64 {
    const HALF_LONG_BITS: u32 = usize::BITS / 2;

    ((((high as u64) << HALF_LONG_BITS) << HALF_LONG_BITS) | low as u64) as i64
}

fn read_into_user_buffer(
    file: &FileDesc,
    uspace: &UserSpaceHandle,
    buf: VirtAddr,
    count: usize,
    offset: Option<usize>,
) -> Result<usize, SysError> {
    let count = clamp_rw_count(count);
    if count == 0 {
        return Ok(0);
    }

    if offset.is_none() {
        let segment = OpenedFileReadUserSegment::new(buf, count);
        if let Some(result) = file.read_user(uspace, core::slice::from_ref(&segment)) {
            return result;
        }
    }

    validate_user_write_buffer(uspace, buf, count)?;

    let kbuf = do_read(file, count, offset)?;
    copy_user_write_buffer(uspace, buf, &kbuf)?;

    Ok(kbuf.len())
}

fn write_from_user_buffer(
    file: &FileDesc,
    uspace: &UserSpaceHandle,
    buf: VirtAddr,
    count: usize,
    offset: Option<usize>,
) -> Result<usize, SysError> {
    let kbuf = copy_user_read_buffer(uspace, buf, count)?;
    do_write(file, &kbuf, offset)
}

fn load_iovecs(
    uspace: &UserSpaceHandle,
    iov: VirtAddr,
    iovcnt: usize,
) -> Result<Vec<CheckedIoVec>, SysError> {
    if iovcnt == 0 {
        return Ok(Vec::new());
    }
    if iovcnt > MAX_IOVEC_CNT {
        return Err(SysError::InvalidArgument);
    }

    let mut raw_iovecs = vec![
        IoVec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        iovcnt
    ];
    {
        let mut guard = uspace.lock();
        let ptr_slice = UserReadSlice::try_new(iov, iovcnt, &mut guard)?;
        ptr_slice.copy_to_slice(&mut raw_iovecs);
    }

    let mut iovecs = Vec::new();
    iovecs
        .try_reserve_exact(iovcnt)
        .map_err(|_| SysError::OutOfMemory)?;

    let mut total = 0usize;

    for raw_iovec in raw_iovecs {
        let len = usize::try_from(raw_iovec.iov_len).map_err(|_| SysError::InvalidArgument)?;
        if len == 0 {
            continue;
        }

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

// Current vectored IO is processed segment-by-segment. That is sufficient for
// now, but it does not try to provide stronger cross-segment atomicity than
// the underlying single-buffer operations already have.
fn read_iovecs(
    file: &FileDesc,
    uspace: &UserSpaceHandle,
    iovecs: &[CheckedIoVec],
    mut offset: Option<usize>,
) -> Result<u64, SysError> {
    if iovecs.is_empty() {
        return Ok(0);
    }

    if offset.is_none() {
        let segments = iovecs
            .iter()
            .map(|iov| OpenedFileReadUserSegment::new(iov.base, iov.len))
            .collect::<Vec<_>>();
        if let Some(result) = file.read_user(uspace, &segments) {
            return result.map(|n| n as u64);
        }
    }

    let mut total = 0u64;

    for iovec in iovecs {
        let read = match read_into_user_buffer(file, uspace, iovec.base, iovec.len, offset) {
            Ok(read) => read,
            Err(err) if total > 0 => return Ok(total),
            Err(err) => return Err(err),
        };

        total += read as u64;

        match advance_offset(&mut offset, read) {
            Ok(()) => {},
            Err(err) if total > 0 => return Ok(total),
            Err(err) => return Err(err),
        }

        if read != iovec.len {
            break;
        }
    }

    Ok(total)
}

fn write_iovecs(
    file: &FileDesc,
    uspace: &UserSpaceHandle,
    iovecs: &[CheckedIoVec],
    mut offset: Option<usize>,
) -> Result<u64, SysError> {
    let mut total = 0u64;

    for iovec in iovecs {
        let written = match write_from_user_buffer(file, uspace, iovec.base, iovec.len, offset) {
            Ok(written) => written,
            Err(err) if total > 0 => return Ok(total),
            Err(err) => return Err(err),
        };

        total += written as u64;

        match advance_offset(&mut offset, written) {
            Ok(()) => {},
            Err(err) if total > 0 => return Ok(total),
            Err(err) => return Err(err),
        }

        if written != iovec.len {
            break;
        }
    }

    Ok(total)
}

fn alloc_zeroed_buffer(len: usize) -> Result<Vec<u8>, SysError> {
    let mut buf = Vec::new();
    buf.try_reserve_exact(len)
        .map_err(|_| SysError::OutOfMemory)?;
    buf.resize(len, 0);
    Ok(buf)
}

fn do_read(file: &FileDesc, count: usize, offset: Option<usize>) -> Result<Vec<u8>, SysError> {
    let mut kbuf = alloc_zeroed_buffer(count)?;
    if count == 0 {
        return Ok(kbuf);
    }

    let len = match offset {
        Some(offset) => file.read_at(offset, &mut kbuf)?,
        None => file.read(&mut kbuf)?,
    };
    kbuf.truncate(len);

    Ok(kbuf)
}

fn do_write(file: &FileDesc, buf: &[u8], offset: Option<usize>) -> Result<usize, SysError> {
    match offset {
        Some(offset) => file.write_at(offset, buf),
        None => file.write(buf),
    }
}

fn validate_user_write_buffer(
    uspace: &UserSpaceHandle,
    buf: VirtAddr,
    count: usize,
) -> Result<(), SysError> {
    let mut guard = uspace.lock();
    let _ = UserWriteSlice::<u8>::try_new(buf, count, &mut guard)?;
    Ok(())
}

fn copy_user_read_buffer(
    uspace: &UserSpaceHandle,
    buf: VirtAddr,
    count: usize,
) -> Result<Vec<u8>, SysError> {
    let count = clamp_rw_count(count);
    let mut kbuf = alloc_zeroed_buffer(count)?;
    if count == 0 {
        return Ok(kbuf);
    }

    let mut guard = uspace.lock();
    let slice = UserReadSlice::try_new(buf, count, &mut guard)?;
    slice.copy_to_slice(&mut kbuf);

    Ok(kbuf)
}

fn copy_user_write_buffer(
    uspace: &UserSpaceHandle,
    buf: VirtAddr,
    src: &[u8],
) -> Result<(), SysError> {
    if src.is_empty() {
        return Ok(());
    }

    let mut guard = uspace.lock();
    let mut slice = UserWriteSlice::try_new(buf, src.len(), &mut guard)?;
    slice.copy_from_slice(src);

    Ok(())
}

fn advance_offset(offset: &mut Option<usize>, delta: usize) -> Result<(), SysError> {
    if let Some(current) = offset.as_mut() {
        *current = current
            .checked_add(delta)
            .ok_or(SysError::InvalidArgument)?;
    }

    Ok(())
}
