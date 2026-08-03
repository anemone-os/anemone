use anemone_abi::fs::linux::ioctl::FIONREAD;

use crate::{
    prelude::*,
    syscall::user_access::UserWritePtr,
    task::{
        files::{FileDescOps, FileStatusFlags, OpenedFileReadUserCtx},
        sig::{
            SigNo, Signal,
            info::{SiCode, SigInfoFields, SigKill},
        },
    },
};

use super::{
    PIPE_ATOMIC_WRITE_BYTES, PipeEndpoint, PipeInner, pipe_state, poll::notify_pipe_poll_routes,
};

fn pipe_read_locked(
    pipe: &mut PipeInner,
    buf: &mut [u8],
) -> (usize, Option<Arc<Vec<super::PipePollRoute>>>) {
    let read = pipe.buf.try_pop_slice(buf);
    let routes = (read > 0).then(|| pipe.tx_poll_routes.clone());
    (read, routes)
}

fn pipe_write_locked(
    pipe: &mut PipeInner,
    buf: &[u8],
) -> (usize, Option<Arc<Vec<super::PipePollRoute>>>) {
    let to_write = pipe.available().min(buf.len());
    let written = pipe.buf.try_push_slice(&buf[..to_write]);
    let routes = (written > 0).then(|| pipe.rx_poll_routes.clone());
    (written, routes)
}

pub(super) fn pipe_rx_read(
    file: &File,
    _pos: &mut usize,
    buf: &mut [u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let endpoint = file
        .prv()
        .cast::<PipeEndpoint>()
        .expect("internal error: pipe file without endpoint private data");
    assert!(
        endpoint.access.can_read(),
        "pipe read reached a write-only endpoint"
    );

    loop {
        let pipe = endpoint.pipe.inner.lock();
        if pipe.buf.is_empty() && pipe.tx_cnt > 0 {
            if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
                return Err(SysError::Again);
            }
            drop(pipe);
            if !endpoint.pipe.wait_until_read_can_continue() {
                return Err(SysError::Interrupted);
            }
            continue;
        }
        if pipe.buf.is_empty() {
            return Ok(0);
        }
        drop(pipe);

        // Never hold the uninterruptible operation mutex while waiting for
        // bytes. A competing reader may consume the observed prefix before we
        // acquire it, so admission must be rechecked under both owners.
        let operation = endpoint.pipe.read_operation.lock();
        let mut pipe = endpoint.pipe.inner.lock();
        if pipe.buf.is_empty() {
            if pipe.tx_cnt == 0 {
                return Ok(0);
            }
            drop(pipe);
            drop(operation);
            continue;
        }

        let (read, routes) = pipe_read_locked(&mut pipe, buf);
        drop(pipe);
        drop(operation);
        if routes.is_some() {
            endpoint.pipe.write_recheck.publish(usize::MAX, false);
        }
        notify_pipe_poll_routes(routes, Some(PollEvent::WRITABLE), "tx", "rx_read");
        return Ok(read);
    }
}

fn pipe_rx_read_user_transaction(ctx: OpenedFileReadUserCtx<'_, '_>) -> Result<usize, SysError> {
    assert!(
        !ctx.notification_suppressed,
        "pipe read transaction must remain an access-notification source"
    );
    let requested = ctx.dst.remaining();
    if requested == 0 {
        return Ok(0);
    }

    let endpoint = ctx
        .file
        .prv()
        .cast::<PipeEndpoint>()
        .expect("internal error: pipe transaction without endpoint private data");
    assert!(
        endpoint.access.can_read(),
        "pipe read transaction reached a write-only endpoint"
    );
    let (operation, staged_len) = loop {
        let pipe = endpoint.pipe.inner.lock();
        if pipe.buf.is_empty() && pipe.tx_cnt > 0 {
            if ctx.status_flags.contains(FileStatusFlags::NONBLOCK) {
                return Err(SysError::Again);
            }
            drop(pipe);
            if !endpoint.pipe.wait_until_read_can_continue() {
                return Err(SysError::Interrupted);
            }
            continue;
        }
        if pipe.buf.is_empty() {
            return Ok(0);
        }
        drop(pipe);

        let operation = endpoint.pipe.read_operation.lock();
        let pipe = endpoint.pipe.inner.lock();
        if pipe.buf.is_empty() {
            if pipe.tx_cnt == 0 {
                return Ok(0);
            }
            drop(pipe);
            drop(operation);
            continue;
        }
        let staged_len = requested.min(pipe.buf.len());
        drop(pipe);
        break (operation, staged_len);
    };

    let mut staged = Vec::new();
    staged
        .try_reserve_exact(staged_len)
        .map_err(|_| SysError::OutOfMemory)?;
    {
        // The RX operation gate excludes every consumer while writers may only
        // append. The selected prefix remains stable without a held spinlock.
        let pipe = endpoint.pipe.inner.lock();
        assert!(
            pipe.buf.len() >= staged_len,
            "pipe staged prefix was consumed outside the RX operation gate"
        );
        staged.extend(pipe.buf.iter().take(staged_len));
    }
    assert_eq!(
        staged.len(),
        staged_len,
        "pipe snapshot did not cover its staged prefix"
    );

    let copied = ctx.dst.write_from_slice(&staged)?;
    assert!(
        copied > 0 && copied <= staged.len(),
        "nonempty pipe copyout made invalid progress"
    );

    let routes = {
        let mut pipe = endpoint.pipe.inner.lock();
        for expected in &staged[..copied] {
            let actual = pipe
                .buf
                .try_pop()
                .expect("pipe staged prefix disappeared before commit");
            assert_eq!(
                actual, *expected,
                "pipe staged prefix changed before read commit"
            );
        }
        (copied > 0).then(|| pipe.tx_poll_routes.clone())
    };

    drop(operation);
    endpoint.pipe.write_recheck.publish(usize::MAX, false);
    notify_pipe_poll_routes(
        routes,
        Some(PollEvent::WRITABLE),
        "tx",
        "rx_read_user_commit",
    );
    Ok(copied)
}

pub(crate) fn pipe_file_desc_ops(mut base: FileDescOps, can_read: bool) -> FileDescOps {
    if can_read {
        base.read_user_transaction = Some(pipe_rx_read_user_transaction);
    }
    base
}

pub(super) fn pipe_tx_write(
    file: &File,
    _pos: &mut usize,
    buf: &[u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let endpoint = file
        .prv()
        .cast::<PipeEndpoint>()
        .expect("internal error: pipe file without endpoint private data");
    assert!(
        endpoint.access.can_write(),
        "pipe write reached a read-only endpoint"
    );
    let mut pipe = endpoint.pipe.inner.lock();

    if pipe.rx_cnt == 0 {
        send_sigpipe();
        return Err(SysError::BrokenPipe);
    }

    let (result, routes) = if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
        let available = pipe.available();
        if available == 0 || (buf.len() <= PIPE_ATOMIC_WRITE_BYTES && available < buf.len()) {
            return Err(SysError::Again);
        }
        let to_write = if buf.len() > PIPE_ATOMIC_WRITE_BYTES {
            available.min(buf.len())
        } else {
            buf.len()
        };
        let (written, routes) = pipe_write_locked(&mut pipe, &buf[..to_write]);
        (Ok(written), routes)
    } else {
        let needs_atomic_write = buf.len() <= PIPE_ATOMIC_WRITE_BYTES;
        while pipe.rx_cnt > 0
            && if needs_atomic_write {
                pipe.available() < buf.len()
            } else {
                pipe.available() == 0
            }
        {
            drop(pipe);
            if !endpoint.pipe.wait_until_write_can_continue(buf.len()) {
                return Err(SysError::Interrupted);
            }
            pipe = endpoint.pipe.inner.lock();
        }

        if pipe.rx_cnt == 0 {
            send_sigpipe();
            (Err(SysError::BrokenPipe), None)
        } else if needs_atomic_write {
            let (written, routes) = pipe_write_locked(&mut pipe, buf);
            assert_eq!(written, buf.len(), "atomic pipe write lost admission");
            (Ok(written), routes)
        } else {
            let to_write = pipe.available().min(buf.len());
            let (written, routes) = pipe_write_locked(&mut pipe, &buf[..to_write]);
            (Ok(written), routes)
        }
    };

    drop(pipe);
    if routes.is_some() {
        endpoint.pipe.read_recheck.publish(usize::MAX, false);
    }
    notify_pipe_poll_routes(routes, Some(PollEvent::READABLE), "rx", "tx_write");
    result
}

fn send_sigpipe() {
    let task = get_current_task();
    task.recv_signal(Signal::new(
        SigNo::SIGPIPE,
        SiCode::Kernel,
        SigInfoFields::Kill(SigKill {
            pid: task.tgid(),
            uid: task.cred().uid.real,
        }),
    ));
}

fn readable_bytes(file: &File) -> Result<usize, SysError> {
    pipe_state(file)
        .map(|pipe| pipe.inner.lock().buf.len())
        .ok_or(SysError::InvalidArgument)
}

fn write_ioctl_value<T: Copy>(ctx: &IoctlCtx<'_>, value: T) -> Result<(), SysError> {
    ctx.uspace().with_usp(|usp| {
        UserWritePtr::<T>::try_new(VirtAddr::new(ctx.arg()), usp)?.write(value)?;
        Ok(())
    })
}

pub(super) fn pipe_ioctl(file: &File, ctx: IoctlCtx<'_>) -> Result<u64, SysError> {
    match ctx.cmd() {
        FIONREAD => {
            let nbytes =
                i32::try_from(readable_bytes(file)?).map_err(|_| SysError::FileTooLarge)?;
            write_ioctl_value(&ctx, nbytes)?;
            Ok(0)
        },
        _ => Err(SysError::UnsupportedIoctl),
    }
}
