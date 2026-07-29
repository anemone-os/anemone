//! In current implementation this is not a real filesystem. It just leverages
//! anonymous inodes to create pipes.
//!
//! Only anonymous pipes are supported for now.
//!
//! TODO: turn to [Event] based implementation.

use anemone_abi::fs::linux::ioctl::FIONREAD;

use crate::{
    fs::{FcntlCtx, FileFcntlCmd, FileFcntlOutcome, FileMode},
    prelude::*,
    syscall::user_access::UserWritePtr,
    task::sig::{
        SigNo, Signal,
        info::{SiCode, SigInfoFields, SigKill},
    },
    utils::{
        any_opaque::{AnyOpaque, NilOpaque},
        ring_buffer::RingBuffer,
    },
};

use super::iomux::PollRoute;

const PIPE_ATOMIC_WRITE_BYTES: usize = PagingArch::PAGE_SIZE_BYTES;
const PIPE_STORAGE_BYTES: usize = PIPE_CAPACITY_PAGES * PIPE_ATOMIC_WRITE_BYTES;
static_assert!(
    PIPE_CAPACITY_PAGES >= 2,
    "pipe_capacity_pages must be at least two so total capacity and the atomic-write threshold remain distinct"
);

#[derive(Clone, Debug)]
struct PipePollRoute {
    route: PollRoute,
    interests: PollEvent,
}

impl PipePollRoute {
    fn new(route: &PollRoute, interests: PollEvent) -> Self {
        Self {
            route: route.clone(),
            interests,
        }
    }
}

fn pipe_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let meta = inode.inode().meta_snapshot();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: meta.nlink,
        uid: meta.uid,
        gid: meta.gid,
        rdev: DeviceId::None,
        size: meta.size,
        atime: meta.atime,
        mtime: meta.mtime,
        ctime: meta.ctime,
    })
}

static PIPE_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| unreachable!(/* pipes have their own open logic */),
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: pipe_get_attr,
};

#[derive(Opaque)]
struct Pipe {
    /// [VecDeque] is definitely a terrible choice for the buffer, cz every byte
    /// read/written will cause metadata update, which is very costly.
    ///
    /// Currently we use a statically allocated ring buffer. In future we may
    /// extend it to support dynamic resizing.
    buf: Box<RingBuffer<u8, PIPE_STORAGE_BYTES>>,
    /// User-visible capacity within the fixed backing store. Keeping this
    /// separate from the one-page atomic-write bound lets a default pipe
    /// remain writable after a small write while a one-page pipe only becomes
    /// writable again after a complete page has been drained.
    capacity: usize,

    rx_cnt: usize,
    tx_cnt: usize,

    /// Copy-on-write registries let predicate transitions clone a snapshot
    /// under the pipe lock without allocating. Subscription builds a fallible
    /// replacement before publication; callbacks and old-registry drop happen
    /// after releasing the pipe lock.
    rx_poll_routes: Arc<Vec<PipePollRoute>>,
    tx_poll_routes: Arc<Vec<PipePollRoute>>,
}

impl Pipe {
    fn new_anonymous() -> (PipeRx, PipeTx) {
        let pipe = Pipe {
            buf: Box::new(RingBuffer::new()),
            capacity: PIPE_STORAGE_BYTES,
            rx_cnt: 1,
            tx_cnt: 1,
            rx_poll_routes: Arc::new(Vec::new()),
            tx_poll_routes: Arc::new(Vec::new()),
        };

        let pipe = Arc::new(SpinLock::new(pipe));

        (PipeRx { pipe: pipe.clone() }, PipeTx { pipe })
    }

    fn capacity(&self) -> usize {
        self.capacity
    }

    fn available(&self) -> usize {
        assert!(self.buf.len() <= self.capacity);
        self.capacity - self.buf.len()
    }
}

#[derive(Opaque)]
struct PipeRx {
    pipe: Arc<SpinLock<Pipe>>,
}

impl Drop for PipeRx {
    fn drop(&mut self) {
        let routes = {
            let mut pipe = self.pipe.lock();
            pipe.rx_cnt -= 1;

            if pipe.rx_cnt == 0 {
                Some(pipe.tx_poll_routes.clone())
            } else {
                None
            }
        };

        notify_pipe_poll_routes(routes, None, "tx", "rx_drop");
    }
}

#[derive(Opaque)]
struct PipeTx {
    pipe: Arc<SpinLock<Pipe>>,
}

impl Drop for PipeTx {
    fn drop(&mut self) {
        let routes = {
            let mut pipe = self.pipe.lock();
            pipe.tx_cnt -= 1;

            if pipe.tx_cnt == 0 {
                Some(pipe.rx_poll_routes.clone())
            } else {
                None
            }
        };

        notify_pipe_poll_routes(routes, None, "rx", "tx_drop");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeEndpointSide {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipeEndpointInfo {
    side: PipeEndpointSide,
}

impl PipeEndpointInfo {
    pub const fn side(self) -> PipeEndpointSide {
        self.side
    }
}

pub fn pipe_endpoint_info(file: &File) -> Option<PipeEndpointInfo> {
    if file.prv().cast::<PipeRx>().is_some() {
        Some(PipeEndpointInfo {
            side: PipeEndpointSide::Read,
        })
    } else if file.prv().cast::<PipeTx>().is_some() {
        Some(PipeEndpointInfo {
            side: PipeEndpointSide::Write,
        })
    } else {
        None
    }
}

pub fn pipe_endpoints_same_pipe(lhs: &File, rhs: &File) -> Result<bool, SysError> {
    let lhs = pipe_state(lhs).ok_or(SysError::InvalidArgument)?;
    let rhs = pipe_state(rhs).ok_or(SysError::InvalidArgument)?;

    // Equality is a one-shot owner-side behavior check for splice/tee errno
    // routing. The syscall layer must not receive or cache the pipe object or a
    // derived pipe id as protocol state.
    Ok(Arc::ptr_eq(lhs, rhs))
}

fn replace_pipe_poll_routes(
    routes: &mut Arc<Vec<PipePollRoute>>,
    route: &PollRoute,
    interests: PollEvent,
) -> Result<(Arc<Vec<PipePollRoute>>, usize), SysError> {
    let retained = routes
        .iter()
        .filter(|entry| !entry.route.is_prunable())
        .count();
    let capacity = retained.checked_add(1).ok_or(SysError::OutOfMemory)?;
    let mut replacement = Vec::new();
    replacement
        .try_reserve(capacity)
        .map_err(|_| SysError::OutOfMemory)?;

    replacement.extend(
        routes
            .iter()
            .filter(|entry| !entry.route.is_prunable())
            .cloned(),
    );
    let pruned = routes.len() - replacement.len();
    replacement.push(PipePollRoute::new(route, interests));

    let replacement = Arc::try_new(replacement).map_err(|_| SysError::OutOfMemory)?;
    Ok((core::mem::replace(routes, replacement), pruned))
}

fn notify_pipe_poll_routes(
    routes: Option<Arc<Vec<PipePollRoute>>>,
    changed: Option<PollEvent>,
    side: &'static str,
    reason: &'static str,
) {
    let Some(routes) = routes else {
        return;
    };

    let mut candidates = 0usize;
    for entry in routes.iter() {
        if changed.is_none_or(|changed| entry.interests.intersects(changed)) {
            entry.route.notify();
            candidates += 1;
        }
    }
    if candidates > 0 {
        kdebugln!(
            "pipe: issued {} {} poll route hints reason={}",
            candidates,
            side,
            reason,
        );
    }
}

fn pipe_rx_revents(pipe: &Pipe, interests: PollEvent) -> PollEvent {
    let mut revents = PollEvent::empty();

    if interests.contains(PollEvent::READABLE) && (!pipe.buf.is_empty() || pipe.tx_cnt == 0) {
        revents |= PollEvent::READABLE;
    }

    if pipe.tx_cnt == 0 {
        revents |= PollEvent::HANG_UP;
    }

    revents
}

fn pipe_tx_revents(pipe: &Pipe, interests: PollEvent) -> PollEvent {
    let mut revents = PollEvent::empty();

    // Linux-compatible writer readiness requires a complete atomic write to
    // fit. Partially draining a one-page pipe may let a smaller nonblocking
    // write succeed, but must not publish a new poll/epoll edge.
    // Read-side notifications remain hints; this source-owned predicate is the
    // single truth used by poll, select, and epoll rechecks.
    if interests.contains(PollEvent::WRITABLE) && pipe.available() >= PIPE_ATOMIC_WRITE_BYTES {
        revents |= PollEvent::WRITABLE;
    }

    if pipe.rx_cnt == 0 {
        revents |= PollEvent::ERROR;
    }

    revents
}

fn pipe_read_locked(pipe: &mut Pipe, buf: &mut [u8]) -> (usize, Option<Arc<Vec<PipePollRoute>>>) {
    let read = pipe.buf.try_pop_slice(buf);
    let routes = if read > 0 {
        Some(pipe.tx_poll_routes.clone())
    } else {
        None
    };
    (read, routes)
}

fn pipe_write_locked(pipe: &mut Pipe, buf: &[u8]) -> (usize, Option<Arc<Vec<PipePollRoute>>>) {
    let to_write = pipe.available().min(buf.len());
    let written = pipe.buf.try_push_slice(&buf[..to_write]);
    let routes = if written > 0 {
        Some(pipe.rx_poll_routes.clone())
    } else {
        None
    };
    (written, routes)
}

fn pipe_rx_read(
    file: &File,
    _pos: &mut usize,
    buf: &mut [u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let rx = file
        .prv()
        .cast::<PipeRx>()
        .expect("internal error: pipe rx file without correct private data");

    let mut pipe = rx.pipe.lock();

    let (result, routes) = if pipe.buf.is_empty() {
        if pipe.tx_cnt == 0 {
            // no tx alive. return EOF.
            (Ok(0), None)
        } else if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
            (Err(SysError::Again), None)
        } else {
            while pipe.buf.is_empty() && pipe.tx_cnt > 0 {
                if get_current_task().has_unmasked_signal() {
                    return Err(SysError::Interrupted);
                }
                drop(pipe);
                yield_now();
                pipe = rx.pipe.lock();
            }

            // out of loop. see what happened.
            if pipe.buf.is_empty() {
                // all tx dead
                (Ok(0), None)
            } else {
                // data available!
                let (read, routes) = pipe_read_locked(&mut pipe, buf);
                (Ok(read), routes)
            }
        }
    } else {
        let (read, routes) = pipe_read_locked(&mut pipe, buf);
        (Ok(read), routes)
    };

    drop(pipe);
    notify_pipe_poll_routes(routes, Some(PollEvent::WRITABLE), "tx", "rx_read");
    result
}

fn pipe_rx_poll(file: &File, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
    let rx = file
        .prv()
        .cast::<PipeRx>()
        .expect("internal error: pipe rx file without correct private data");

    let mut pipe = rx.pipe.lock();
    if !request.is_register() {
        return Ok(PollRegisterResult::Ready(pipe_rx_revents(
            &pipe,
            request.interests(),
        )));
    }

    let Some(route) = request.route() else {
        let revents = pipe_rx_revents(&pipe, request.interests());
        return Ok(if revents.is_empty() {
            PollRegisterResult::Unsupported
        } else {
            PollRegisterResult::Ready(revents)
        });
    };
    let (previous_routes, pruned) =
        replace_pipe_poll_routes(&mut pipe.rx_poll_routes, route, request.interests())?;
    let revents = pipe_rx_revents(&pipe, request.interests());
    let queue_len = pipe.rx_poll_routes.len();
    drop(pipe);
    drop(previous_routes);

    kdebugln!(
        "pipe: subscribed rx poll interests={:?} queue_len={} pruned={}",
        request.interests(),
        queue_len,
        pruned,
    );

    Ok(PollRegisterResult::Subscribed(revents))
}

fn pipe_tx_write(
    file: &File,
    _pos: &mut usize,
    buf: &[u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let tx = file
        .prv()
        .cast::<PipeTx>()
        .expect("internal error: pipe tx file without correct private data");

    let mut pipe = tx.pipe.lock();

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
            if get_current_task().has_unmasked_signal() {
                return Err(SysError::Interrupted);
            }
            drop(pipe);
            yield_now();
            pipe = tx.pipe.lock();
        }

        if pipe.rx_cnt == 0 {
            send_sigpipe();
            (Err(SysError::BrokenPipe), None)
        } else if needs_atomic_write {
            let (written, routes) = pipe_write_locked(&mut pipe, buf);
            assert!(
                written == buf.len(),
                "we should have enough space to write all data"
            );
            (Ok(written), routes)
        } else {
            let to_write = pipe.available().min(buf.len());
            let (written, routes) = pipe_write_locked(&mut pipe, &buf[..to_write]);
            (Ok(written), routes)
        }
    };

    drop(pipe);
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

fn with_pipe_endpoint<T>(
    file: &File,
    f: impl FnOnce(&Arc<SpinLock<Pipe>>, Option<&PipeRx>, Option<&PipeTx>) -> T,
) -> Option<T> {
    if let Some(rx) = file.prv().cast::<PipeRx>() {
        Some(f(&rx.pipe, Some(rx), None))
    } else {
        file.prv()
            .cast::<PipeTx>()
            .map(|tx| f(&tx.pipe, None, Some(tx)))
    }
}

fn pipe_state(file: &File) -> Option<&Arc<SpinLock<Pipe>>> {
    if let Some(rx) = file.prv().cast::<PipeRx>() {
        Some(&rx.pipe)
    } else {
        file.prv().cast::<PipeTx>().map(|tx| &tx.pipe)
    }
}

pub(super) fn display_name(file: &File) -> Option<PathBuf> {
    with_pipe_endpoint(file, |_, _, _| {
        let target = format!("pipe:[{}]", file.inode().ino().get());
        PathBuf::from(target.as_str())
    })
}

fn readable_bytes(file: &File) -> Result<usize, SysError> {
    with_pipe_endpoint(file, |pipe, _, _| pipe.lock().buf.len()).ok_or(SysError::InvalidArgument)
}

fn write_ioctl_value<T: Copy>(ctx: &IoctlCtx<'_>, value: T) -> Result<(), SysError> {
    ctx.uspace().with_usp(|usp| {
        UserWritePtr::<T>::try_new(VirtAddr::new(ctx.arg()), usp)?.write(value)?;
        Ok(())
    })
}

fn pipe_ioctl(file: &File, ctx: IoctlCtx<'_>) -> Result<u64, SysError> {
    match ctx.cmd() {
        FIONREAD => {
            let nbytes = readable_bytes(file)?;
            let nbytes = i32::try_from(nbytes).map_err(|_| SysError::FileTooLarge)?;
            write_ioctl_value(&ctx, nbytes)?;
            Ok(0)
        },
        _ => Err(SysError::UnsupportedIoctl),
    }
}

fn pipe_set_capacity(
    pipe: &SpinLock<Pipe>,
    requested: u64,
) -> Result<(usize, Option<Arc<Vec<PipePollRoute>>>), SysError> {
    if requested > i32::MAX as u64 {
        return Err(SysError::InvalidArgument);
    }

    let mut pipe = pipe.lock();
    let requested = requested as usize;
    let rounded = if requested == 0 {
        PagingArch::PAGE_SIZE_BYTES
    } else {
        align_up_power_of_2!(requested, PagingArch::PAGE_SIZE_BYTES)
    };

    if rounded < pipe.buf.len() {
        return Err(SysError::Busy);
    }
    if rounded > PIPE_STORAGE_BYTES {
        return Err(SysError::PermissionDenied);
    }

    let was_writable = pipe.available() >= PIPE_ATOMIC_WRITE_BYTES;
    pipe.capacity = rounded;
    let became_writable = !was_writable && pipe.available() >= PIPE_ATOMIC_WRITE_BYTES;
    let routes = became_writable.then(|| pipe.tx_poll_routes.clone());
    Ok((pipe.capacity(), routes))
}

fn pipe_fcntl(file: &File, ctx: &FcntlCtx) -> Result<FileFcntlOutcome, SysError> {
    let pipe = pipe_state(file).expect("internal error: pipe fcntl without pipe private data");

    match ctx.cmd() {
        FileFcntlCmd::GetPipeSize => Ok(FileFcntlOutcome::Handled(pipe.lock().capacity() as u64)),
        FileFcntlCmd::SetPipeSize => {
            let (capacity, routes) = pipe_set_capacity(pipe, ctx.arg())?;
            notify_pipe_poll_routes(routes, Some(PollEvent::WRITABLE), "tx", "set_capacity");
            Ok(FileFcntlOutcome::Handled(capacity as u64))
        },
    }
}

fn pipe_tx_poll(file: &File, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
    let tx = file
        .prv()
        .cast::<PipeTx>()
        .expect("internal error: pipe tx file without correct private data");

    let mut pipe = tx.pipe.lock();
    if !request.is_register() {
        return Ok(PollRegisterResult::Ready(pipe_tx_revents(
            &pipe,
            request.interests(),
        )));
    }

    let Some(route) = request.route() else {
        let revents = pipe_tx_revents(&pipe, request.interests());
        return Ok(if revents.is_empty() {
            PollRegisterResult::Unsupported
        } else {
            PollRegisterResult::Ready(revents)
        });
    };
    let (previous_routes, pruned) =
        replace_pipe_poll_routes(&mut pipe.tx_poll_routes, route, request.interests())?;
    let revents = pipe_tx_revents(&pipe, request.interests());
    let queue_len = pipe.tx_poll_routes.len();
    drop(pipe);
    drop(previous_routes);

    kdebugln!(
        "pipe: subscribed tx poll interests={:?} queue_len={} pruned={}",
        request.interests(),
        queue_len,
        pruned,
    );

    Ok(PollRegisterResult::Subscribed(revents))
}

static PIPE_RX_FILE_OPS: FileOps = FileOps {
    read: pipe_rx_read,
    write: |_, _, _, _| Err(SysError::NotSupported),
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::NotSupported),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: pipe_rx_poll,
    fcntl: Some(pipe_fcntl),
    ioctl: pipe_ioctl,
};

static PIPE_TX_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::NotSupported),
    write: pipe_tx_write,
    read_at: |_, _, _, _| Err(SysError::NotSupported),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: pipe_tx_poll,
    fcntl: Some(pipe_fcntl),
    ioctl: pipe_ioctl,
};

pub struct OpenedPipe {
    pub rx: File,
    pub tx: File,
}

/// Creates an anonymous pipe and returns the read and write ends of it.
pub fn create_anonymous_pipe() -> Result<OpenedPipe, SysError> {
    let inode = anony_new_inode(InodeType::Fifo, &PIPE_INODE_OPS, NilOpaque::new())?;

    let (rx, tx) = Pipe::new_anonymous();

    let rx = anony_open_with(
        &inode,
        OpenedFile::with_mode(&PIPE_RX_FILE_OPS, FileMode::STREAM, AnyOpaque::new(rx)),
    )?;

    let tx = anony_open_with(
        &inode,
        OpenedFile::with_mode(&PIPE_TX_FILE_OPS, FileMode::STREAM, AnyOpaque::new(tx)),
    )?;

    Ok(OpenedPipe { rx, tx })
}

// TODO: named pipes. i.e. fifo. we'll do this after we refactor current inode
// ops vtable.
