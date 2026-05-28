//! In current implementation this is not a real filesystem. It just leverages
//! anonymous inodes to create pipes.
//!
//! Only anonymous pipes are supported for now.
//!
//! TODO: turn to [Event] based implementation.

use crate::{
    prelude::*,
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
}

impl Pipe {
    fn new_anonymous() -> (PipeRx, PipeTx) {
        let pipe = Pipe {
            buf: Box::new(RingBuffer::new()),
            rx_cnt: 1,
            tx_cnt: 1,
        };

        let pipe = Arc::new(SpinLock::new(pipe));

        (
            PipeRx {
                pipe: pipe.clone(),
                poll_waiters: SpinLock::new(Vec::new()),
                nonblock: AtomicBool::new(false),
            },
            PipeTx {
                pipe,
                poll_waiters: SpinLock::new(Vec::new()),
                nonblock: AtomicBool::new(false),
            },
        )
    }

    fn capacity(&self) -> usize {
        self.buf.len() + self.buf.available()
    }
}

#[derive(Opaque)]
struct PipeRx {
    pipe: Arc<SpinLock<Pipe>>,
    poll_waiters: SpinLock<Vec<Weak<PollWaiter>>>,
    nonblock: AtomicBool,
}

impl Drop for PipeRx {
    fn drop(&mut self) {
        let mut pipe = self.pipe.lock();
        pipe.rx_cnt -= 1;

        // when we turned into wait queue based implementation, we should wake
        // up all waiting tx when rx_cnt becomes 0.
    }
}

#[derive(Opaque)]
struct PipeTx {
    pipe: Arc<SpinLock<Pipe>>,
    poll_waiters: SpinLock<Vec<Weak<PollWaiter>>>,
    nonblock: AtomicBool,
}

impl Drop for PipeTx {
    fn drop(&mut self) {
        let mut pipe = self.pipe.lock();
        pipe.tx_cnt -= 1;

        // when we turned into wait queue based implementation, we should wake
        // up all waiting rx when tx_cnt becomes 0.
    }
}

fn pipe_rx_read(file: &File, _pos: &mut usize, buf: &mut [u8]) -> Result<usize, SysError> {
    let rx = file
        .prv()
        .cast::<PipeRx>()
        .expect("internal error: pipe rx file without correct private data");

    let mut pipe = rx.pipe.lock();

    if pipe.buf.is_empty() {
        if pipe.tx_cnt == 0 {
            // no tx alive. return EOF.
            Ok(0)
        } else if rx.nonblock.load(Ordering::Relaxed) {
            Err(SysError::Again)
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
                Ok(0)
            } else {
                // data available!
                let read = pipe.buf.try_pop_slice(buf);
                Ok(read)
            }
        }
    } else {
        let read = pipe.buf.try_pop_slice(buf);
        Ok(read)
    }
}

fn pipe_rx_poll(file: &File, request: &PollRequest<'_>) -> Result<PollEvent, SysError> {
    let rx = file
        .prv()
        .cast::<PipeRx>()
        .expect("internal error: pipe rx file without correct private data");

    let pipe = rx.pipe.lock();
    let mut rx_poll_waiters = rx.poll_waiters.lock();

    let mut revents = PollEvent::empty();

    if request.interests().contains(PollEvent::READABLE) {
        if !pipe.buf.is_empty() || pipe.tx_cnt == 0 {
            revents |= PollEvent::READABLE;
        }
    }

    if pipe.tx_cnt == 0 {
        revents |= PollEvent::HANG_UP;
    }

    Ok(revents)
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

    if tx.nonblock.load(Ordering::Relaxed) {
        let available = pipe.buf.available();
        if available == 0 || (buf.len() <= PIPE_CAPACITY_BYTES && available < buf.len()) {
            return Err(SysError::Again);
        }

        let to_write = if buf.len() > PIPE_CAPACITY_BYTES {
            available.min(buf.len())
        } else {
            buf.len()
        };
        return Ok(pipe.buf.try_push_slice(&buf[..to_write]));
    }

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
        Err(SysError::BrokenPipe)
    } else if needs_atomic_write {
        let written = pipe.buf.try_push_slice(buf);
        assert!(
            written == buf.len(),
            "we should have enough space to write all data"
        );
        Ok(written)
    } else {
        let to_write = pipe.buf.available().min(buf.len());
        Ok(pipe.buf.try_push_slice(&buf[..to_write]))
    }
}

fn send_sigpipe() {
    let task = get_current_task();
    task.recv_signal(Signal::new(
        SigNo::SIGPIPE,
        SiCode::Kernel,
        SigInfoFields::Kill(SigKill {
            pid: task.tgid(),
            uid: 0,
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

pub(super) fn readable_bytes(file: &File) -> Result<usize, SysError> {
    with_pipe_endpoint(file, |pipe, _, _| pipe.lock().buf.len()).ok_or(SysError::InvalidArgument)
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

fn pipe_tx_poll(file: &File, request: &PollRequest<'_>) -> Result<PollEvent, SysError> {
    let tx = file
        .prv()
        .cast::<PipeTx>()
        .expect("internal error: pipe tx file without correct private data");

    let pipe = tx.pipe.lock();
    let mut tx_poll_waiters = tx.poll_waiters.lock();

    let mut revents = PollEvent::empty();

    if request.interests().contains(PollEvent::WRITABLE) {
        if !pipe.buf.is_full() {
            revents |= PollEvent::WRITABLE;
        }
    }

    if pipe.rx_cnt == 0 {
        revents |= PollEvent::ERROR;
    }

    Ok(revents)
}

static PIPE_RX_FILE_OPS: FileOps = FileOps {
    read: pipe_rx_read,
    write: |_, _, _| Err(SysError::NotSupported),
    validate_seek: |_, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: pipe_rx_poll,
};

static PIPE_TX_FILE_OPS: FileOps = FileOps {
    read: |_, _, _| Err(SysError::NotSupported),
    write: pipe_tx_write,
    validate_seek: |_, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: pipe_tx_poll,
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
