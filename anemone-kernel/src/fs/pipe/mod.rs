//! In current implementation this is not a real filesystem. It just leverages
//! anonymous inodes to create pipes.
//!
//! Only anonymous pipes are supported for now.

mod capacity;
mod io;
mod poll;

use crate::{
    fs::FileMode,
    prelude::*,
    utils::{
        any_opaque::{AnyOpaque, NilOpaque},
        ring_buffer::HeapRingBuffer,
    },
};

use poll::{PipePollRoute, notify_pipe_poll_routes};

pub(crate) use io::pipe_rx_file_desc_ops;

const PIPE_ATOMIC_WRITE_BYTES: usize = PagingArch::PAGE_SIZE_BYTES;
const PIPE_DEFAULT_CAPACITY_BYTES: usize = PIPE_CAPACITY_PAGES
    .checked_mul(PIPE_ATOMIC_WRITE_BYTES)
    .expect("pipe default capacity must fit usize");
const PIPE_MAX_CAPACITY_BYTES: usize = PIPE_MAX_CAPACITY_PAGES
    .checked_mul(PIPE_ATOMIC_WRITE_BYTES)
    .expect("pipe maximum capacity must fit usize");

static_assert!(
    PIPE_CAPACITY_PAGES >= 2,
    "pipe_capacity_pages must be at least two so default capacity and the atomic-write threshold remain distinct"
);
static_assert!(
    PIPE_CAPACITY_PAGES.is_power_of_two(),
    "pipe_capacity_pages must be a power of two"
);
static_assert!(
    PIPE_MAX_CAPACITY_PAGES.is_power_of_two(),
    "pipe_max_capacity_pages must be a power of two"
);
static_assert!(
    PIPE_MAX_CAPACITY_PAGES >= PIPE_CAPACITY_PAGES,
    "pipe_max_capacity_pages must be at least pipe_capacity_pages"
);

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
    make_node: reject_make_node,
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
    inner: SpinLock<PipeInner>,
    /// Direct read/write waiters only use these events as recheck hints. The
    /// authoritative predicates remain under `inner`, and every publication
    /// happens after releasing that lock.
    read_recheck: Event,
    write_recheck: Event,
}

struct PipeInner {
    /// The ring owns both readable bytes and the single authoritative capacity.
    /// Normal I/O never allocates; capacity replacement is committed under the
    /// same lock as every read/write predicate.
    buf: HeapRingBuffer<u8>,
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
    fn new_anonymous() -> Result<(PipeRx, PipeTx), SysError> {
        let buf = HeapRingBuffer::try_new(PIPE_DEFAULT_CAPACITY_BYTES)
            .map_err(|_| SysError::OutOfMemory)?;
        let rx_poll_routes = Arc::try_new(Vec::new()).map_err(|_| SysError::OutOfMemory)?;
        let tx_poll_routes = Arc::try_new(Vec::new()).map_err(|_| SysError::OutOfMemory)?;
        let pipe = Arc::try_new(Pipe {
            inner: SpinLock::new(PipeInner {
                buf,
                rx_cnt: 1,
                tx_cnt: 1,
                rx_poll_routes,
                tx_poll_routes,
            }),
            read_recheck: Event::new(),
            write_recheck: Event::new(),
        })
        .map_err(|_| SysError::OutOfMemory)?;

        Ok((
            PipeRx {
                pipe: pipe.clone(),
                operation: Mutex::new(()),
            },
            PipeTx { pipe },
        ))
    }

    fn wait_until_read_can_continue(&self) -> bool {
        self.read_recheck.listen(false, || {
            let pipe = self.inner.lock();
            !pipe.buf.is_empty() || pipe.tx_cnt == 0
        })
    }

    fn wait_until_write_can_continue(&self, requested: usize) -> bool {
        let needs_atomic_write = requested <= PIPE_ATOMIC_WRITE_BYTES;
        self.write_recheck.listen(false, || {
            let pipe = self.inner.lock();
            pipe.rx_cnt == 0
                || if needs_atomic_write {
                    pipe.available() >= requested
                } else {
                    pipe.available() > 0
                }
        })
    }
}

impl PipeInner {
    fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    fn available(&self) -> usize {
        self.buf.available()
    }
}

#[derive(Opaque)]
struct PipeRx {
    pipe: Arc<Pipe>,
    /// Serializes consumption by kernel-buffer reads and direct-user read
    /// transactions. Pipe bytes remain owned solely by `PipeInner::buf`; this
    /// gate only keeps a staged prefix stable until copyout commits its exact
    /// count.
    operation: Mutex<()>,
}

impl Drop for PipeRx {
    fn drop(&mut self) {
        let routes = {
            let mut pipe = self.pipe.inner.lock();
            pipe.rx_cnt -= 1;
            (pipe.rx_cnt == 0).then(|| pipe.tx_poll_routes.clone())
        };

        if routes.is_some() {
            self.pipe.write_recheck.publish(usize::MAX, false);
        }
        notify_pipe_poll_routes(routes, None, "tx", "rx_drop");
    }
}

#[derive(Opaque)]
struct PipeTx {
    pipe: Arc<Pipe>,
}

impl Drop for PipeTx {
    fn drop(&mut self) {
        let routes = {
            let mut pipe = self.pipe.inner.lock();
            pipe.tx_cnt -= 1;
            (pipe.tx_cnt == 0).then(|| pipe.rx_poll_routes.clone())
        };

        if routes.is_some() {
            self.pipe.read_recheck.publish(usize::MAX, false);
        }
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

fn with_pipe_endpoint<T>(
    file: &File,
    f: impl FnOnce(&Arc<Pipe>, Option<&PipeRx>, Option<&PipeTx>) -> T,
) -> Option<T> {
    if let Some(rx) = file.prv().cast::<PipeRx>() {
        Some(f(&rx.pipe, Some(rx), None))
    } else {
        file.prv()
            .cast::<PipeTx>()
            .map(|tx| f(&tx.pipe, None, Some(tx)))
    }
}

fn pipe_state(file: &File) -> Option<&Arc<Pipe>> {
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

static PIPE_RX_FILE_OPS: FileOps = FileOps {
    read: io::pipe_rx_read,
    write: |_, _, _, _| Err(SysError::NotSupported),
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::NotSupported),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: poll::pipe_rx_poll,
    fcntl: Some(capacity::pipe_fcntl),
    ioctl: io::pipe_ioctl,
};

static PIPE_TX_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::NotSupported),
    write: io::pipe_tx_write,
    read_at: |_, _, _, _| Err(SysError::NotSupported),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: poll::pipe_tx_poll,
    fcntl: Some(capacity::pipe_fcntl),
    ioctl: io::pipe_ioctl,
};

pub struct OpenedPipe {
    pub rx: File,
    pub tx: File,
}

/// Creates an anonymous pipe and returns the read and write ends of it.
pub fn create_anonymous_pipe() -> Result<OpenedPipe, SysError> {
    // Backing allocation precedes every inode/endpoint publication, so ENOMEM
    // cannot leave a partially visible anonymous pipe.
    let (rx, tx) = Pipe::new_anonymous()?;
    let inode = anony_new_inode(InodeType::Fifo, &PIPE_INODE_OPS, NilOpaque::new())?;

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

#[cfg(feature = "kunit")]
mod kunits {
    use alloc::{vec, vec::Vec};

    use super::*;

    #[kunit]
    fn wrapped_bytes_survive_grow_busy_shrink_and_legal_shrink() {
        let (rx, _tx) = Pipe::new_anonymous().unwrap();
        let pipe = &rx.pipe;
        let initial: Vec<u8> = (0..7_000).map(|index| (index % 251) as u8).collect();
        let appended: Vec<u8> = (0..2_500).map(|index| (index % 239) as u8).collect();
        {
            let mut inner = pipe.inner.lock();
            assert_eq!(inner.buf.try_push_slice(&initial), initial.len());
            let mut discarded = vec![0; 6_000];
            assert_eq!(inner.buf.try_pop_slice(&mut discarded), discarded.len());
            assert_eq!(inner.buf.try_push_slice(&appended), appended.len());
        }

        let expected: Vec<_> = pipe.inner.lock().buf.iter().collect();
        assert_eq!(expected.len(), 3_500);
        assert_eq!(
            capacity::resize_pipe(pipe, (4 * PIPE_ATOMIC_WRITE_BYTES) as u64)
                .unwrap()
                .0,
            4 * PIPE_ATOMIC_WRITE_BYTES
        );
        assert_eq!(pipe.inner.lock().buf.iter().collect::<Vec<_>>(), expected);

        {
            let mut inner = pipe.inner.lock();
            assert_eq!(inner.buf.try_push_slice(&vec![0xaa; 1_000]), 1_000);
        }
        let before_busy: Vec<_> = pipe.inner.lock().buf.iter().collect();
        assert_eq!(
            capacity::resize_pipe(pipe, PIPE_ATOMIC_WRITE_BYTES as u64).unwrap_err(),
            SysError::Busy
        );
        assert_eq!(pipe.inner.lock().capacity(), 4 * PIPE_ATOMIC_WRITE_BYTES);
        assert_eq!(
            pipe.inner.lock().buf.iter().collect::<Vec<_>>(),
            before_busy
        );

        let after_drain = before_busy[1_000..].to_vec();
        let mut discarded = vec![0; 1_000];
        assert_eq!(pipe.inner.lock().buf.try_pop_slice(&mut discarded), 1_000);
        assert_eq!(
            capacity::resize_pipe(pipe, 0).unwrap().0,
            PIPE_ATOMIC_WRITE_BYTES
        );
        assert_eq!(
            pipe.inner.lock().buf.iter().collect::<Vec<_>>(),
            after_drain
        );
    }
}
