use crate::{
    fs::{FcntlCtx, FileFcntlCmd, FileFcntlOutcome},
    prelude::*,
    utils::ring_buffer::HeapRingBuffer,
};

use super::{
    PIPE_ATOMIC_WRITE_BYTES, PIPE_DEFAULT_CAPACITY_BYTES, PIPE_MAX_CAPACITY_BYTES, Pipe,
    PipePollRoute, pipe_state, poll::notify_pipe_poll_routes,
};

fn normalize_pipe_capacity(requested: u64) -> Result<usize, SysError> {
    if requested > i32::MAX as u64 {
        return Err(SysError::InvalidArgument);
    }
    if requested == 0 {
        return Ok(PIPE_ATOMIC_WRITE_BYTES);
    }

    let requested = usize::try_from(requested).map_err(|_| SysError::InvalidArgument)?;
    let pages = requested
        .checked_add(PIPE_ATOMIC_WRITE_BYTES - 1)
        .ok_or(SysError::InvalidArgument)?
        / PIPE_ATOMIC_WRITE_BYTES;
    let pages = pages
        .checked_next_power_of_two()
        .ok_or(SysError::InvalidArgument)?;
    pages
        .checked_mul(PIPE_ATOMIC_WRITE_BYTES)
        .ok_or(SysError::InvalidArgument)
}

pub(super) fn resize_pipe(
    pipe: &Pipe,
    requested: u64,
) -> Result<(usize, bool, Option<Arc<Vec<PipePollRoute>>>), SysError> {
    let capacity = normalize_pipe_capacity(requested)?;
    if capacity > PIPE_MAX_CAPACITY_BYTES {
        return Err(SysError::PermissionDenied);
    }

    {
        let inner = pipe.inner.lock();
        if capacity == inner.capacity() {
            return Ok((capacity, false, None));
        }
        if capacity < inner.buf.len() {
            return Err(SysError::Busy);
        }
    }

    // Allocation happens outside the pipe lock. Until the later swap, the old
    // ring remains the only published capacity and byte-order truth.
    let mut candidate = HeapRingBuffer::try_new(capacity).map_err(|_| SysError::OutOfMemory)?;
    let mut inner = pipe.inner.lock();
    if capacity == inner.capacity() {
        drop(inner);
        drop(candidate);
        return Ok((capacity, false, None));
    }
    if capacity < inner.buf.len() {
        drop(inner);
        drop(candidate);
        return Err(SysError::Busy);
    }

    let old_capacity = inner.capacity();
    let was_writable = inner.available() >= PIPE_ATOMIC_WRITE_BYTES;
    let unread = inner.buf.len();
    let (first, second) = inner.buf.readable_slices();
    let copied = candidate.try_push_slice(first) + candidate.try_push_slice(second);
    assert_eq!(copied, unread, "pipe resize candidate lost readable bytes");

    // This swap is the linearization point for both capacity and FIFO content.
    let old = core::mem::replace(&mut inner.buf, candidate);
    let increased = capacity > old_capacity;
    let became_writable = !was_writable && inner.available() >= PIPE_ATOMIC_WRITE_BYTES;
    let routes = became_writable.then(|| inner.tx_poll_routes.clone());
    drop(inner);
    // Heap deallocation and any allocator-side work stay outside the pipe lock.
    drop(old);
    Ok((capacity, increased, routes))
}

pub(super) fn pipe_fcntl(file: &File, ctx: &FcntlCtx) -> Result<FileFcntlOutcome, SysError> {
    let pipe = pipe_state(file).expect("internal error: pipe fcntl without pipe private data");
    match ctx.cmd() {
        FileFcntlCmd::GetPipeSize => Ok(FileFcntlOutcome::Handled(
            pipe.inner.lock().capacity() as u64
        )),
        FileFcntlCmd::SetPipeSize => {
            let (capacity, increased, routes) = resize_pipe(pipe, ctx.arg())?;
            if increased {
                pipe.write_recheck.publish(usize::MAX, false);
            }
            notify_pipe_poll_routes(routes, Some(PollEvent::WRITABLE), "tx", "set_capacity");
            Ok(FileFcntlOutcome::Handled(capacity as u64))
        },
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn normalization_matches_pipe_abi() {
        assert_eq!(normalize_pipe_capacity(0), Ok(PIPE_ATOMIC_WRITE_BYTES));
        assert_eq!(normalize_pipe_capacity(1), Ok(PIPE_ATOMIC_WRITE_BYTES));
        assert_eq!(
            normalize_pipe_capacity((PIPE_ATOMIC_WRITE_BYTES + 1) as u64),
            Ok(2 * PIPE_ATOMIC_WRITE_BYTES)
        );
        assert_eq!(
            normalize_pipe_capacity((2 * PIPE_ATOMIC_WRITE_BYTES + 1) as u64),
            Ok(4 * PIPE_ATOMIC_WRITE_BYTES)
        );
        assert_eq!(
            normalize_pipe_capacity(i32::MAX as u64 + 1),
            Err(SysError::InvalidArgument)
        );
    }

    #[kunit]
    fn maximum_and_same_size_are_classified_before_allocation() {
        let (rx, _tx) = Pipe::new_anonymous().unwrap();
        assert_eq!(
            resize_pipe(&rx.pipe, (PIPE_MAX_CAPACITY_BYTES + 1) as u64).unwrap_err(),
            SysError::PermissionDenied
        );
        let (capacity, increased, routes) =
            resize_pipe(&rx.pipe, PIPE_DEFAULT_CAPACITY_BYTES as u64).unwrap();
        assert_eq!(capacity, PIPE_DEFAULT_CAPACITY_BYTES);
        assert!(!increased);
        assert!(routes.is_none());
    }
}
