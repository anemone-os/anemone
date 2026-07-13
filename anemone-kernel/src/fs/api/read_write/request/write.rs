use alloc::vec::Vec;

use crate::{
    fs::{
        UserBufferSegment, UserBufferSource,
        fanotify::{FanMask, notify_opened_file_event},
    },
    prelude::{user_access::UserReadSlice, *},
    task::files::FileDesc,
};

use super::{CheckedIoVec, RequestPosition, alloc_zeroed_buffer, clamp_rw_count};

pub(in crate::fs::api::read_write) struct WriteRequest<'a> {
    file: &'a FileDesc,
    uspace: &'a UserSpaceHandle,
    buffer: WriteBuffer<'a>,
    position: RequestPosition,
}

enum WriteBuffer<'a> {
    Single { buf: VirtAddr, count: usize },
    Vectored(&'a [CheckedIoVec]),
}

enum WriteNotifyPolicy {
    Modify,
}

impl<'a> WriteRequest<'a> {
    pub(in crate::fs::api::read_write) fn single(
        file: &'a FileDesc,
        uspace: &'a UserSpaceHandle,
        buf: VirtAddr,
        count: usize,
    ) -> Self {
        Self {
            file,
            uspace,
            buffer: WriteBuffer::Single { buf, count },
            position: RequestPosition::Sequential,
        }
    }

    pub(in crate::fs::api::read_write) fn vectored(
        file: &'a FileDesc,
        uspace: &'a UserSpaceHandle,
        iovecs: &'a [CheckedIoVec],
    ) -> Self {
        Self {
            file,
            uspace,
            buffer: WriteBuffer::Vectored(iovecs),
            position: RequestPosition::Sequential,
        }
    }

    pub(in crate::fs::api::read_write) fn at(mut self, offset: usize) -> Self {
        self.position = RequestPosition::Positioned(offset);
        self
    }

    pub(in crate::fs::api::read_write) fn execute(self) -> Result<u64, SysError> {
        match self.buffer {
            WriteBuffer::Single { buf, count } => self.execute_single(buf, count),
            WriteBuffer::Vectored(iovecs) => self.execute_vectored(iovecs),
        }
    }

    fn execute_single(self, buf: VirtAddr, count: usize) -> Result<u64, SysError> {
        let count = clamp_rw_count(count);
        let segment = UserBufferSegment::new(buf, count);
        let mut src = UserBufferSource::new(self.uspace, core::slice::from_ref(&segment));
        if let Some(result) = write_user_source(self.file, &mut src, self.position) {
            let written = result? as u64;
            return finalize_write(self.file, written, WriteNotifyPolicy::Modify);
        }

        let written =
            write_fallback_segment(self.file, self.uspace, buf, count, self.position)? as u64;
        finalize_write(self.file, written, WriteNotifyPolicy::Modify)
    }

    // Segment helpers return only progress or error. The request owns the
    // aggregate partial-success and final notification decision for the whole
    // syscall.
    fn execute_vectored(self, iovecs: &[CheckedIoVec]) -> Result<u64, SysError> {
        if iovecs.is_empty() {
            return Ok(0);
        }

        let mut position = self.position;
        if self.file.vfs_file().has_write_user_at() {
            let segments = iovecs
                .iter()
                .map(|iov| UserBufferSegment::new(iov.base, iov.len))
                .collect::<Vec<_>>();
            let mut src = UserBufferSource::new(self.uspace, &segments);
            if let Some(result) = write_user_source(self.file, &mut src, position) {
                let written = result? as u64;
                return finalize_write(self.file, written, WriteNotifyPolicy::Modify);
            }
        }

        let mut total = 0u64;

        for iovec in iovecs {
            let written = match write_fallback_segment(
                self.file,
                self.uspace,
                iovec.base,
                iovec.len,
                position,
            ) {
                Ok(written) => written,
                Err(_) if total > 0 => {
                    return finalize_write(self.file, total, WriteNotifyPolicy::Modify);
                },
                Err(err) => return Err(err),
            };

            total += written as u64;

            match position.advance(written) {
                Ok(()) => {},
                Err(_) if total > 0 => {
                    return finalize_write(self.file, total, WriteNotifyPolicy::Modify);
                },
                Err(err) => return Err(err),
            }

            if written != iovec.len {
                break;
            }
        }

        finalize_write(self.file, total, WriteNotifyPolicy::Modify)
    }
}

fn write_user_source(
    file: &FileDesc,
    src: &mut UserBufferSource<'_>,
    position: RequestPosition,
) -> Option<Result<usize, SysError>> {
    match position {
        RequestPosition::Positioned(offset) => file.write_user_at(offset, src),
        RequestPosition::Sequential => file.write_user(src),
    }
}

fn write_fallback_segment(
    file: &FileDesc,
    uspace: &UserSpaceHandle,
    buf: VirtAddr,
    count: usize,
    position: RequestPosition,
) -> Result<usize, SysError> {
    let kbuf = copy_user_read_buffer(uspace, buf, count)?;
    do_write(file, &kbuf, position.offset())
}

fn do_write(file: &FileDesc, buf: &[u8], offset: Option<usize>) -> Result<usize, SysError> {
    match offset {
        Some(offset) => file.write_at(offset, buf),
        None => file.write(buf),
    }
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

fn finalize_write(file: &FileDesc, bytes: u64, policy: WriteNotifyPolicy) -> Result<u64, SysError> {
    match policy {
        WriteNotifyPolicy::Modify => notify_write_success(file, bytes),
    }
    Ok(bytes)
}

fn notify_write_success(file: &FileDesc, bytes: u64) {
    if bytes > 0 {
        notify_opened_file_event(file, FanMask::MODIFY);
    }
}
