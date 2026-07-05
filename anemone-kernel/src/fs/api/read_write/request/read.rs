use alloc::vec::Vec;

use crate::{
    fs::{
        UserBufferSegment, UserBufferSink,
        fanotify::{FanMask, notify_opened_file_event},
    },
    prelude::{user_access::UserWriteSlice, *},
    task::files::FileDesc,
};

use super::{CheckedIoVec, RequestPosition, alloc_zeroed_buffer, clamp_rw_count};

pub(in crate::fs::api::read_write) struct ReadRequest<'a> {
    file: &'a FileDesc,
    uspace: &'a UserSpaceHandle,
    buffer: ReadBuffer<'a>,
    position: RequestPosition,
}

enum ReadBuffer<'a> {
    Single { buf: VirtAddr, count: usize },
    Vectored(&'a [CheckedIoVec]),
}

enum ReadNotifyPolicy {
    Access,
    AccessIfDescriptionAllows,
}

impl<'a> ReadRequest<'a> {
    pub(in crate::fs::api::read_write) fn single(
        file: &'a FileDesc,
        uspace: &'a UserSpaceHandle,
        buf: VirtAddr,
        count: usize,
    ) -> Self {
        Self {
            file,
            uspace,
            buffer: ReadBuffer::Single { buf, count },
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
            buffer: ReadBuffer::Vectored(iovecs),
            position: RequestPosition::Sequential,
        }
    }

    pub(in crate::fs::api::read_write) fn at(mut self, offset: usize) -> Self {
        self.position = RequestPosition::Positioned(offset);
        self
    }

    pub(in crate::fs::api::read_write) fn execute(self) -> Result<u64, SysError> {
        match self.buffer {
            ReadBuffer::Single { buf, count } => self.execute_single(buf, count),
            ReadBuffer::Vectored(iovecs) => self.execute_vectored(iovecs),
        }
    }

    fn execute_single(self, buf: VirtAddr, count: usize) -> Result<u64, SysError> {
        let count = clamp_rw_count(count);
        if count == 0 {
            return Ok(0);
        }

        let segment = UserBufferSegment::new(buf, count);
        let mut dst = UserBufferSink::new(self.uspace, core::slice::from_ref(&segment));
        if let Some((result, policy)) = read_user_sink(self.file, &mut dst, self.position) {
            let read = result? as u64;
            return finalize_read(self.file, read, policy);
        }

        let read = read_fallback_segment(self.file, self.uspace, buf, count, self.position)? as u64;
        finalize_read(self.file, read, ReadNotifyPolicy::Access)
    }

    // Current vectored IO is processed segment-by-segment once the aggregate
    // direct-user path is unavailable. That preserves the existing
    // visible-bytes-first partial rule without making segment helpers decide
    // fanotify notification policy.
    fn execute_vectored(self, iovecs: &[CheckedIoVec]) -> Result<u64, SysError> {
        if iovecs.is_empty() {
            return Ok(0);
        }

        let mut position = self.position;
        if position.is_sequential() || self.file.vfs_file().has_read_user_at() {
            let segments = iovecs
                .iter()
                .map(|iov| UserBufferSegment::new(iov.base, iov.len))
                .collect::<Vec<_>>();
            let mut dst = UserBufferSink::new(self.uspace, &segments);
            if let Some((result, policy)) = read_user_sink(self.file, &mut dst, position) {
                let read = result? as u64;
                return finalize_read(self.file, read, policy);
            }
        }

        let mut total = 0u64;

        for iovec in iovecs {
            let read = match read_fallback_segment(
                self.file,
                self.uspace,
                iovec.base,
                iovec.len,
                position,
            ) {
                Ok(read) => read,
                Err(_) if total > 0 => {
                    return finalize_read(self.file, total, ReadNotifyPolicy::Access);
                },
                Err(err) => return Err(err),
            };

            total += read as u64;

            match position.advance(read) {
                Ok(()) => {},
                Err(_) if total > 0 => {
                    return finalize_read(self.file, total, ReadNotifyPolicy::Access);
                },
                Err(err) => return Err(err),
            }

            if read != iovec.len {
                break;
            }
        }

        finalize_read(self.file, total, ReadNotifyPolicy::Access)
    }
}

fn read_user_sink(
    file: &FileDesc,
    dst: &mut UserBufferSink<'_>,
    position: RequestPosition,
) -> Option<(Result<usize, SysError>, ReadNotifyPolicy)> {
    match position {
        RequestPosition::Positioned(offset) => file
            .read_user_at(offset, dst)
            .map(|result| (result, ReadNotifyPolicy::Access)),
        RequestPosition::Sequential => {
            if let Some(result) = file.read_user_transaction(dst) {
                return Some((result, ReadNotifyPolicy::AccessIfDescriptionAllows));
            }

            // `None` is the only direct-user fallback signal. Once a backend
            // installs `read_user_at`, its errors are user-visible results, not
            // a request to retry through the kernel-buffer trampoline.
            file.read_user(dst)
                .map(|result| (result, ReadNotifyPolicy::Access))
        },
    }
}

fn read_fallback_segment(
    file: &FileDesc,
    uspace: &UserSpaceHandle,
    buf: VirtAddr,
    count: usize,
    position: RequestPosition,
) -> Result<usize, SysError> {
    validate_user_write_buffer(uspace, buf, count)?;

    let kbuf = do_read(file, count, position.offset())?;
    copy_user_write_buffer(uspace, buf, &kbuf)?;

    Ok(kbuf.len())
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

fn validate_user_write_buffer(
    uspace: &UserSpaceHandle,
    buf: VirtAddr,
    count: usize,
) -> Result<(), SysError> {
    let mut guard = uspace.lock();
    let _ = UserWriteSlice::<u8>::try_new(buf, count, &mut guard)?;
    Ok(())
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

fn finalize_read(file: &FileDesc, bytes: u64, policy: ReadNotifyPolicy) -> Result<u64, SysError> {
    match policy {
        ReadNotifyPolicy::Access => notify_read_success(file, bytes),
        ReadNotifyPolicy::AccessIfDescriptionAllows => {
            if file.notify_read_user_access() {
                notify_read_success(file, bytes);
            }
        },
    }
    Ok(bytes)
}

fn notify_read_success(file: &FileDesc, bytes: u64) {
    if bytes > 0 {
        notify_opened_file_event(file, FanMask::ACCESS);
    }
}
