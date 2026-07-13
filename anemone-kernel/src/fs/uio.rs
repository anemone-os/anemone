use crate::{
    prelude::*,
    syscall::user_access::{UserReadSlice, UserWriteSlice},
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct UserBufferSegment {
    base: VirtAddr,
    len: usize,
}

impl UserBufferSegment {
    pub(crate) const fn new(base: VirtAddr, len: usize) -> Self {
        Self { base, len }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct UserBufferMark {
    cursor: UserBufferCursor,
    done: usize,
}

#[derive(Debug, Clone, Copy)]
struct UserBufferCursor {
    segment: usize,
    offset: usize,
}

impl UserBufferCursor {
    const fn start() -> Self {
        Self {
            segment: 0,
            offset: 0,
        }
    }
}

// A sink is a short-lived linear capability for copying file bytes into the
// current syscall's userspace buffers. Backends may advance it through the copy
// helpers, but must not save the cursor, segments, or user-space handle beyond
// the call that received it.
pub struct UserBufferSink<'a> {
    uspace: &'a UserSpaceHandle,
    segments: &'a [UserBufferSegment],
    cursor: UserBufferCursor,
    written: usize,
}

impl<'a> UserBufferSink<'a> {
    pub(crate) fn new(uspace: &'a UserSpaceHandle, segments: &'a [UserBufferSegment]) -> Self {
        Self {
            uspace,
            segments,
            cursor: UserBufferCursor::start(),
            written: 0,
        }
    }

    pub(crate) fn remaining(&self) -> usize {
        remaining_from(self.segments, self.cursor)
    }

    // Ordinary file I/O uses partial-progress semantics: once any bytes have
    // reached userspace, a later bad page or iovec turns this helper into a
    // short copy instead of erasing already-visible progress.
    pub(crate) fn write_from_slice(&mut self, src: &[u8]) -> Result<usize, SysError> {
        let mut copied = 0usize;
        if src.is_empty() {
            return Ok(0);
        }

        let mut guard = self.uspace.lock();
        while copied < src.len() {
            let Some((cursor, addr, available)) = current_range(self.segments, self.cursor)? else {
                break;
            };
            let copy_len = (src.len() - copied)
                .min(available)
                .min(bytes_until_page_end(addr));

            match UserWriteSlice::<u8>::try_new(addr, copy_len, &mut guard) {
                Ok(mut dst) => dst.copy_from_slice(&src[copied..copied + copy_len]),
                Err(err) if copied > 0 => return Ok(copied),
                Err(err) => return Err(err),
            }

            self.cursor = advance_cursor(self.segments, cursor, copy_len);
            self.written = self
                .written
                .checked_add(copy_len)
                .expect("user-buffer sink progress overflow");
            copied += copy_len;
        }

        Ok(copied)
    }

    pub(crate) fn write_zeros(&mut self, len: usize) -> Result<usize, SysError> {
        let zeros = [0u8; 256];
        let mut copied = 0usize;

        while copied < len {
            let copy_len = (len - copied).min(zeros.len());
            match self.write_from_slice(&zeros[..copy_len]) {
                Ok(0) => break,
                Ok(n) => copied += n,
                Err(err) if copied > 0 => return Ok(copied),
                Err(err) => return Err(err),
            }
        }

        Ok(copied)
    }

    // Exact record copyout is only for transaction records such as fanotify
    // metadata, where publishing half a record would desynchronize the paired
    // side effect such as fd reservation/commit. Ordinary vectored I/O must keep
    // using partial-progress helpers instead of whole-record prevalidation.
    pub(crate) fn exact_record<'b>(&'b mut self) -> UserRecordSink<'a, 'b> {
        UserRecordSink { inner: self }
    }

    // Marks let VFS wrappers derive externally visible read progress from this
    // cursor's own movement, avoiding a second byte-count truth source in the
    // backend hook.
    pub(crate) fn mark(&self) -> UserBufferMark {
        UserBufferMark {
            cursor: self.cursor,
            done: self.written,
        }
    }

    pub(crate) fn bytes_since(&self, mark: UserBufferMark) -> usize {
        assert!(
            self.written >= mark.done,
            "user-buffer sink mark from a later cursor"
        );
        self.written - mark.done
    }

    fn write_exact_record(&mut self, record: &[u8]) -> Result<(), SysError> {
        if record.is_empty() {
            return Ok(());
        }
        if self.remaining() < record.len() {
            return Err(SysError::InvalidArgument);
        }

        let mut guard = self.uspace.lock();
        // Keep the user-space lock across both validation and copy. The exact
        // path promises that a later bad iovec cannot leave a half record; the
        // stable lock window makes the successfully validated ranges the same
        // ranges that receive the record bytes below.
        let mut cursor = self.cursor;
        let mut checked = 0usize;
        while checked < record.len() {
            let Some((normalized, addr, available)) = current_range(self.segments, cursor)? else {
                return Err(SysError::InvalidArgument);
            };
            let len = (record.len() - checked).min(available);
            let _ = UserWriteSlice::<u8>::try_new(addr, len, &mut guard)?;
            cursor = advance_cursor(self.segments, normalized, len);
            checked += len;
        }
        assert!(
            checked == record.len(),
            "exact user-buffer record validation was short"
        );

        let mut cursor = self.cursor;
        let mut copied = 0usize;
        while copied < record.len() {
            let Some((normalized, addr, available)) = current_range(self.segments, cursor)? else {
                unreachable!("validated exact user-buffer record lost its target range");
            };
            let len = (record.len() - copied).min(available);
            let mut dst = UserWriteSlice::<u8>::try_new(addr, len, &mut guard)
                .expect("validated exact user-buffer record became invalid");
            dst.copy_from_slice(&record[copied..copied + len]);
            cursor = advance_cursor(self.segments, normalized, len);
            copied += len;
        }
        assert!(
            copied == record.len(),
            "exact user-buffer record copy was short"
        );

        self.cursor = cursor;
        self.written = self
            .written
            .checked_add(record.len())
            .expect("user-buffer sink progress overflow");
        Ok(())
    }
}

// A source is the write-side userspace capability. Its consumed byte count
// means only "copied out of userspace"; the file-visible commit count is owned
// by the write hook and may be smaller.
pub(crate) struct UserBufferSource<'a> {
    uspace: &'a UserSpaceHandle,
    segments: &'a [UserBufferSegment],
    cursor: UserBufferCursor,
    consumed: usize,
}

impl<'a> UserBufferSource<'a> {
    pub(crate) fn new(uspace: &'a UserSpaceHandle, segments: &'a [UserBufferSegment]) -> Self {
        Self {
            uspace,
            segments,
            cursor: UserBufferCursor::start(),
            consumed: 0,
        }
    }

    pub(crate) fn remaining(&self) -> usize {
        remaining_from(self.segments, self.cursor)
    }

    pub(crate) fn copy_into_slice(&mut self, dst: &mut [u8]) -> Result<usize, SysError> {
        let mut copied = 0usize;
        if dst.is_empty() {
            return Ok(0);
        }

        let mut guard = self.uspace.lock();
        while copied < dst.len() {
            let Some((cursor, addr, available)) = current_range(self.segments, self.cursor)? else {
                break;
            };
            let copy_len = (dst.len() - copied)
                .min(available)
                .min(bytes_until_page_end(addr));

            match UserReadSlice::<u8>::try_new(addr, copy_len, &mut guard) {
                Ok(src) => src.copy_to_slice(&mut dst[copied..copied + copy_len]),
                Err(err) if copied > 0 => return Ok(copied),
                Err(err) => return Err(err),
            }

            self.cursor = advance_cursor(self.segments, cursor, copy_len);
            self.consumed = self
                .consumed
                .checked_add(copy_len)
                .expect("user-buffer source progress overflow");
            copied += copy_len;
        }

        Ok(copied)
    }

    pub(crate) fn mark(&self) -> UserBufferMark {
        UserBufferMark {
            cursor: self.cursor,
            done: self.consumed,
        }
    }

    // After a write hook commits fewer bytes than it copied from userspace,
    // discard the speculative suffix so later accounting and offset advancement
    // are based only on file-visible progress.
    pub(crate) fn keep_prefix_from(&mut self, mark: UserBufferMark, committed: usize) {
        let copied = self.bytes_since(mark);
        assert!(
            committed <= copied,
            "file-visible progress exceeds copied user-buffer source bytes"
        );
        self.cursor = advance_cursor(self.segments, mark.cursor, committed);
        self.consumed = mark
            .done
            .checked_add(committed)
            .expect("user-buffer source progress overflow");
    }

    pub(crate) fn bytes_since(&self, mark: UserBufferMark) -> usize {
        assert!(
            self.consumed >= mark.done,
            "user-buffer source mark from a later cursor"
        );
        self.consumed - mark.done
    }
}

pub(crate) struct UserRecordSink<'a, 'b> {
    inner: &'b mut UserBufferSink<'a>,
}

impl<'a, 'b> UserRecordSink<'a, 'b> {
    pub(crate) fn write_exact(&mut self, record: &[u8]) -> Result<(), SysError> {
        self.inner.write_exact_record(record)
    }
}

fn remaining_from(segments: &[UserBufferSegment], cursor: UserBufferCursor) -> usize {
    let cursor = normalize_cursor(segments, cursor);
    if cursor.segment >= segments.len() {
        return 0;
    }

    let mut remaining = segments[cursor.segment].len - cursor.offset;
    for segment in &segments[cursor.segment + 1..] {
        remaining = remaining
            .checked_add(segment.len)
            .expect("checked user-buffer segments overflow");
    }
    remaining
}

fn current_range(
    segments: &[UserBufferSegment],
    cursor: UserBufferCursor,
) -> Result<Option<(UserBufferCursor, VirtAddr, usize)>, SysError> {
    let cursor = normalize_cursor(segments, cursor);
    if cursor.segment >= segments.len() {
        return Ok(None);
    }

    let segment = segments[cursor.segment];
    let base = segment
        .base
        .get()
        .checked_add(cursor.offset as u64)
        .ok_or(SysError::BadAddress)?;
    Ok(Some((
        cursor,
        VirtAddr::new(base),
        segment.len - cursor.offset,
    )))
}

fn normalize_cursor(
    segments: &[UserBufferSegment],
    mut cursor: UserBufferCursor,
) -> UserBufferCursor {
    while cursor.segment < segments.len() && cursor.offset >= segments[cursor.segment].len {
        cursor.segment += 1;
        cursor.offset = 0;
    }
    cursor
}

fn advance_cursor(
    segments: &[UserBufferSegment],
    cursor: UserBufferCursor,
    mut delta: usize,
) -> UserBufferCursor {
    assert!(
        delta <= remaining_from(segments, cursor),
        "user-buffer cursor advanced beyond remaining bytes"
    );

    let mut cursor = normalize_cursor(segments, cursor);
    while delta > 0 {
        let available = segments[cursor.segment].len - cursor.offset;
        let step = delta.min(available);
        cursor.offset += step;
        delta -= step;
        if cursor.offset == segments[cursor.segment].len {
            cursor.segment += 1;
            cursor.offset = 0;
        }
    }
    normalize_cursor(segments, cursor)
}

fn bytes_until_page_end(addr: VirtAddr) -> usize {
    let page_offset = addr.get() as usize & (PagingArch::PAGE_SIZE_BYTES - 1);
    PagingArch::PAGE_SIZE_BYTES - page_offset
}
