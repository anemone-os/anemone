//! In current implementation this is not a real filesystem. It just leverages
//! anonymous inodes to create pipes.
//!
//! Only anonymous pipes are supported for now.
//!
//! TODO: turn to [Event] based implementation.

use anemone_abi::fs::linux::ioctl::FIONREAD;

use crate::{
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

const PIPE_CAPACITY_BYTES: usize = PagingArch::PAGE_SIZE_BYTES;

#[derive(Clone, Debug)]
struct PipePollTrigger {
    trigger: LatchTrigger,
    interests: PollEvent,
}

impl PipePollTrigger {
    fn new(trigger: &LatchTrigger, interests: PollEvent) -> Self {
        Self {
            trigger: trigger.clone(),
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
    buf: Box<RingBuffer<u8, { PagingArch::PAGE_SIZE_BYTES }>>,

    rx_cnt: usize,
    tx_cnt: usize,

    rx_poll_triggers: Vec<PipePollTrigger>,
    tx_poll_triggers: Vec<PipePollTrigger>,
}

impl Pipe {
    fn new_anonymous() -> (PipeRx, PipeTx) {
        let pipe = Pipe {
            buf: Box::new(RingBuffer::new()),
            rx_cnt: 1,
            tx_cnt: 1,
            rx_poll_triggers: Vec::new(),
            tx_poll_triggers: Vec::new(),
        };

        let pipe = Arc::new(SpinLock::new(pipe));

        (
            PipeRx {
                pipe: pipe.clone(),
                nonblock: AtomicBool::new(false),
            },
            PipeTx {
                pipe,
                nonblock: AtomicBool::new(false),
            },
        )
    }

    fn capacity(&self) -> usize {
        self.buf.len() + self.buf.available()
    }

    fn prune_rx_poll_triggers(&mut self) {
        prune_pipe_poll_triggers(&mut self.rx_poll_triggers, "rx");
    }

    fn prune_tx_poll_triggers(&mut self) {
        prune_pipe_poll_triggers(&mut self.tx_poll_triggers, "tx");
    }

    fn detach_rx_poll_triggers(&mut self, reason: &'static str) -> Vec<PipePollTrigger> {
        self.prune_rx_poll_triggers();
        let detached = core::mem::take(&mut self.rx_poll_triggers);
        if !detached.is_empty() {
            kdebugln!(
                "pipe: detach rx poll triggers reason={} count={}",
                reason,
                detached.len(),
            );
        }
        detached
    }

    fn detach_tx_poll_triggers(&mut self, reason: &'static str) -> Vec<PipePollTrigger> {
        self.prune_tx_poll_triggers();
        let detached = core::mem::take(&mut self.tx_poll_triggers);
        if !detached.is_empty() {
            kdebugln!(
                "pipe: detach tx poll triggers reason={} count={}",
                reason,
                detached.len(),
            );
        }
        detached
    }
}

#[derive(Opaque)]
struct PipeRx {
    pipe: Arc<SpinLock<Pipe>>,
    nonblock: AtomicBool,
}

impl Drop for PipeRx {
    fn drop(&mut self) {
        let detached = {
            let mut pipe = self.pipe.lock();
            pipe.rx_cnt -= 1;

            if pipe.rx_cnt == 0 {
                pipe.detach_tx_poll_triggers("rx_drop")
            } else {
                Vec::new()
            }
        };

        trigger_pipe_poll_triggers(detached, "tx", "rx_drop");
    }
}

#[derive(Opaque)]
struct PipeTx {
    pipe: Arc<SpinLock<Pipe>>,
    nonblock: AtomicBool,
}

impl Drop for PipeTx {
    fn drop(&mut self) {
        let detached = {
            let mut pipe = self.pipe.lock();
            pipe.tx_cnt -= 1;

            if pipe.tx_cnt == 0 {
                pipe.detach_rx_poll_triggers("tx_drop")
            } else {
                Vec::new()
            }
        };

        trigger_pipe_poll_triggers(detached, "rx", "tx_drop");
    }
}

fn prune_pipe_poll_triggers(queue: &mut Vec<PipePollTrigger>, side: &'static str) {
    let before = queue.len();
    queue.retain(|entry| !entry.trigger.is_prunable());
    let pruned = before - queue.len();
    if pruned > 0 {
        kdebugln!("pipe: pruned {} {} poll triggers", pruned, side);
    }
}

fn trigger_pipe_poll_triggers(
    triggers: Vec<PipePollTrigger>,
    side: &'static str,
    reason: &'static str,
) {
    for entry in triggers {
        kdebugln!(
            "pipe: trigger {} poll wait={:#x} interests={:?} reason={}",
            side,
            entry.trigger.wait_id(),
            entry.interests,
            reason,
        );
        entry.trigger.trigger();
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

    if interests.contains(PollEvent::WRITABLE) && !pipe.buf.is_full() {
        revents |= PollEvent::WRITABLE;
    }

    if pipe.rx_cnt == 0 {
        revents |= PollEvent::ERROR;
    }

    revents
}

fn pipe_read_locked(
    pipe: &mut Pipe,
    buf: &mut [u8],
    reason: &'static str,
) -> (usize, Vec<PipePollTrigger>) {
    let read = pipe.buf.try_pop_slice(buf);
    let detached = if read > 0 {
        pipe.detach_tx_poll_triggers(reason)
    } else {
        Vec::new()
    };
    (read, detached)
}

fn pipe_write_locked(
    pipe: &mut Pipe,
    buf: &[u8],
    reason: &'static str,
) -> (usize, Vec<PipePollTrigger>) {
    let written = pipe.buf.try_push_slice(buf);
    let detached = if written > 0 {
        pipe.detach_rx_poll_triggers(reason)
    } else {
        Vec::new()
    };
    (written, detached)
}

fn pipe_rx_read(file: &File, _pos: &mut usize, buf: &mut [u8]) -> Result<usize, SysError> {
    let rx = file
        .prv()
        .cast::<PipeRx>()
        .expect("internal error: pipe rx file without correct private data");

    let mut pipe = rx.pipe.lock();

    let (result, detached) = if pipe.buf.is_empty() {
        if pipe.tx_cnt == 0 {
            // no tx alive. return EOF.
            (Ok(0), Vec::new())
        } else if rx.nonblock.load(Ordering::Relaxed) {
            (Err(SysError::Again), Vec::new())
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
                (Ok(0), Vec::new())
            } else {
                // data available!
                let (read, detached) = pipe_read_locked(&mut pipe, buf, "rx_read");
                (Ok(read), detached)
            }
        }
    } else {
        let (read, detached) = pipe_read_locked(&mut pipe, buf, "rx_read");
        (Ok(read), detached)
    };

    drop(pipe);
    trigger_pipe_poll_triggers(detached, "tx", "rx_read");
    result
}

fn pipe_rx_poll(
    file: &File,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let rx = file
        .prv()
        .cast::<PipeRx>()
        .expect("internal error: pipe rx file without correct private data");

    let mut pipe = rx.pipe.lock();
    let revents = pipe_rx_revents(&pipe, request.interests());
    if !revents.is_empty() || !request.is_register() {
        return Ok(PollRegisterResult::Ready(revents));
    }

    let trigger = request
        .trigger()
        .expect("register request disappeared after is_register");
    pipe.prune_rx_poll_triggers();
    pipe.rx_poll_triggers
        .push(PipePollTrigger::new(trigger, request.interests()));

    kdebugln!(
        "pipe: armed rx poll wait={:#x} interests={:?} queue_len={}",
        trigger.wait_id(),
        request.interests(),
        pipe.rx_poll_triggers.len(),
    );

    Ok(PollRegisterResult::Armed)
}

fn pipe_tx_write(file: &File, _pos: &mut usize, buf: &[u8]) -> Result<usize, SysError> {
    let tx = file
        .prv()
        .cast::<PipeTx>()
        .expect("internal error: pipe tx file without correct private data");

    let mut pipe = tx.pipe.lock();

    if pipe.rx_cnt == 0 {
        send_sigpipe();
        return Err(SysError::BrokenPipe);
    }

    let (result, detached) = if tx.nonblock.load(Ordering::Relaxed) {
        let available = pipe.buf.available();
        if available == 0 || (buf.len() <= PIPE_CAPACITY_BYTES && available < buf.len()) {
            return Err(SysError::Again);
        }

        let to_write = if buf.len() > PIPE_CAPACITY_BYTES {
            available.min(buf.len())
        } else {
            buf.len()
        };
        let (written, detached) = pipe_write_locked(&mut pipe, &buf[..to_write], "tx_write");
        (Ok(written), detached)
    } else {
        let needs_atomic_write = buf.len() <= PIPE_CAPACITY_BYTES;

        while pipe.rx_cnt > 0
            && if needs_atomic_write {
                pipe.buf.available() < buf.len()
            } else {
                pipe.buf.available() == 0
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
            (Err(SysError::BrokenPipe), Vec::new())
        } else if needs_atomic_write {
            let (written, detached) = pipe_write_locked(&mut pipe, buf, "tx_write");
            assert!(
                written == buf.len(),
                "we should have enough space to write all data"
            );
            (Ok(written), detached)
        } else {
            let to_write = pipe.buf.available().min(buf.len());
            let (written, detached) = pipe_write_locked(&mut pipe, &buf[..to_write], "tx_write");
            (Ok(written), detached)
        }
    };

    drop(pipe);
    trigger_pipe_poll_triggers(detached, "rx", "tx_write");
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

pub(super) fn update_nonblock(file: &File, nonblock: bool) {
    let _ = with_pipe_endpoint(file, |_, rx, tx| {
        if let Some(rx) = rx {
            rx.nonblock.store(nonblock, Ordering::Relaxed);
        }
        if let Some(tx) = tx {
            tx.nonblock.store(nonblock, Ordering::Relaxed);
        }
    });
}

fn readable_bytes(file: &File) -> Result<usize, SysError> {
    with_pipe_endpoint(file, |pipe, _, _| pipe.lock().buf.len()).ok_or(SysError::InvalidArgument)
}

fn write_ioctl_value<T: Copy>(ctx: &IoctlCtx<'_>, value: T) -> Result<(), SysError> {
    ctx.uspace().with_usp(|usp| {
        UserWritePtr::<T>::try_new(VirtAddr::new(ctx.arg()), usp)?.write(value);
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

pub(super) fn capacity(file: &File) -> Result<usize, SysError> {
    with_pipe_endpoint(file, |pipe, _, _| pipe.lock().capacity()).ok_or(SysError::InvalidArgument)
}

pub(super) fn set_capacity(file: &File, requested: u64) -> Result<usize, SysError> {
    if requested > i32::MAX as u64 {
        return Err(SysError::InvalidArgument);
    }

    with_pipe_endpoint(file, |pipe, _, _| {
        let pipe = pipe.lock();
        let requested = requested as usize;
        let rounded = if requested == 0 {
            PagingArch::PAGE_SIZE_BYTES
        } else {
            align_up_power_of_2!(requested, PagingArch::PAGE_SIZE_BYTES)
        };

        if rounded < pipe.buf.len() {
            Err(SysError::Busy)
        } else if rounded <= pipe.capacity() {
            Ok(pipe.capacity())
        } else {
            Err(SysError::PermissionDenied)
        }
    })
    .ok_or(SysError::InvalidArgument)?
}

fn pipe_tx_poll(
    file: &File,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let tx = file
        .prv()
        .cast::<PipeTx>()
        .expect("internal error: pipe tx file without correct private data");

    let mut pipe = tx.pipe.lock();
    let revents = pipe_tx_revents(&pipe, request.interests());
    if !revents.is_empty() || !request.is_register() {
        return Ok(PollRegisterResult::Ready(revents));
    }

    let trigger = request
        .trigger()
        .expect("register request disappeared after is_register");
    pipe.prune_tx_poll_triggers();
    pipe.tx_poll_triggers
        .push(PipePollTrigger::new(trigger, request.interests()));

    kdebugln!(
        "pipe: armed tx poll wait={:#x} interests={:?} queue_len={}",
        trigger.wait_id(),
        request.interests(),
        pipe.tx_poll_triggers.len(),
    );

    Ok(PollRegisterResult::Armed)
}

static PIPE_RX_FILE_OPS: FileOps = FileOps {
    read: pipe_rx_read,
    write: |_, _, _| Err(SysError::NotSupported),
    validate_seek: |_, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: pipe_rx_poll,
    ioctl: pipe_ioctl,
};

static PIPE_TX_FILE_OPS: FileOps = FileOps {
    read: |_, _, _| Err(SysError::NotSupported),
    write: pipe_tx_write,
    validate_seek: |_, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: pipe_tx_poll,
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
        OpenedFile {
            file_ops: &PIPE_RX_FILE_OPS,
            prv: AnyOpaque::new(rx),
        },
    )?;

    let tx = anony_open_with(
        &inode,
        OpenedFile {
            file_ops: &PIPE_TX_FILE_OPS,
            prv: AnyOpaque::new(tx),
        },
    )?;

    Ok(OpenedPipe { rx, tx })
}

// TODO: named pipes. i.e. fifo. we'll do this after we refactor current inode
// ops vtable.
