//! Pipe data plane and anonymous/named FIFO endpoint lifecycle.

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

pub(crate) use io::pipe_file_desc_ops;

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
    /// Serializes every reader's kernel-buffer and direct-user consumption in
    /// this session. Named FIFO opens create distinct endpoints, so this gate
    /// must live with the shared byte owner rather than any one endpoint.
    read_operation: Mutex<()>,
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
    /// Monotonic admission generations remember an overlapping partner even if
    /// it retires before a blocking opener is scheduled. They are session-local
    /// protocol state and disappear with the Pipe.
    rx_generation: u64,
    tx_generation: u64,

    /// Copy-on-write registries let predicate transitions clone a snapshot
    /// under the pipe lock without allocating. Subscription builds a fallible
    /// replacement before publication; callbacks and old-registry drop happen
    /// after releasing the pipe lock.
    rx_poll_routes: Arc<Vec<PipePollRoute>>,
    tx_poll_routes: Arc<Vec<PipePollRoute>>,
}

/// The resident FIFO inode's only runtime rendezvous slot.
///
/// The weak reference does not own the Pipe session. Endpoints, pending opens,
/// and in-flight file operations provide the strong capabilities; when they are
/// gone a later open replaces the stale weak reference with a fresh empty Pipe.
pub(in crate::fs) struct FifoAnchor {
    current: SpinLock<Weak<Pipe>>,
}

impl FifoAnchor {
    pub(in crate::fs) fn new() -> Self {
        Self {
            current: SpinLock::new(Weak::new()),
        }
    }

    fn get_or_create(&self) -> Result<Arc<Pipe>, SysError> {
        if let Some(pipe) = self.current.lock().upgrade() {
            return Ok(pipe);
        }

        // Candidate allocation stays outside the anchor lock. A concurrent
        // winner is reused; the losing empty candidate has no participants,
        // bytes, routes, or externally visible side effects.
        let candidate = Pipe::try_new()?;
        let mut current = self.current.lock();
        if let Some(pipe) = current.upgrade() {
            return Ok(pipe);
        }
        *current = Arc::downgrade(&candidate);
        Ok(candidate)
    }

    fn install(&self, pipe: &Arc<Pipe>) {
        let mut current = self.current.lock();
        assert!(
            current.upgrade().is_none(),
            "fresh FIFO inode already has a live Pipe session"
        );
        *current = Arc::downgrade(pipe);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PipeAccess {
    Read,
    Write,
    ReadWrite,
}

impl PipeAccess {
    const fn can_read(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }

    const fn can_write(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::fs) enum FifoOpenAccess {
    Read,
    Write,
    ReadWrite,
}

impl From<FifoOpenAccess> for PipeAccess {
    fn from(access: FifoOpenAccess) -> Self {
        match access {
            FifoOpenAccess::Read => Self::Read,
            FifoOpenAccess::Write => Self::Write,
            FifoOpenAccess::ReadWrite => Self::ReadWrite,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::fs) struct FifoOpenContext {
    access: FifoOpenAccess,
    nonblock: bool,
}

impl FifoOpenContext {
    pub(in crate::fs) const fn new(access: FifoOpenAccess, nonblock: bool) -> Self {
        Self { access, nonblock }
    }
}

#[derive(Clone, Copy)]
struct PartnerGeneration {
    rx: u64,
    tx: u64,
}

impl Pipe {
    fn try_new() -> Result<Arc<Self>, SysError> {
        let buf = HeapRingBuffer::try_new(PIPE_DEFAULT_CAPACITY_BYTES)
            .map_err(|_| SysError::OutOfMemory)?;
        let rx_poll_routes = Arc::try_new(Vec::new()).map_err(|_| SysError::OutOfMemory)?;
        let tx_poll_routes = Arc::try_new(Vec::new()).map_err(|_| SysError::OutOfMemory)?;
        Arc::try_new(Pipe {
            inner: SpinLock::new(PipeInner {
                buf,
                rx_cnt: 0,
                tx_cnt: 0,
                rx_generation: 0,
                tx_generation: 0,
                rx_poll_routes,
                tx_poll_routes,
            }),
            read_operation: Mutex::new(()),
            read_recheck: Event::new(),
            write_recheck: Event::new(),
        })
        .map_err(|_| SysError::OutOfMemory)
    }

    fn new_anonymous() -> Result<(PipeEndpoint, PipeEndpoint), SysError> {
        let pipe = Self::try_new()?;
        let rx = PipeAdmission::begin(pipe.clone(), PipeAccess::Read).commit();
        let tx = PipeAdmission::begin(pipe, PipeAccess::Write).commit();
        Ok((rx, tx))
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

    fn partner_arrived(&self, access: PipeAccess, observed: PartnerGeneration) -> bool {
        let pipe = self.inner.lock();
        match access {
            PipeAccess::Read => pipe.tx_cnt > 0 || pipe.tx_generation != observed.tx,
            PipeAccess::Write => pipe.rx_cnt > 0 || pipe.rx_generation != observed.rx,
            PipeAccess::ReadWrite => true,
        }
    }

    fn wait_for_partner(&self, access: PipeAccess, observed: PartnerGeneration) -> bool {
        match access {
            PipeAccess::Read => self
                .read_recheck
                .listen(false, || self.partner_arrived(access, observed)),
            PipeAccess::Write => self
                .write_recheck
                .listen(false, || self.partner_arrived(access, observed)),
            PipeAccess::ReadWrite => true,
        }
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
struct PipeEndpoint {
    pipe: Arc<Pipe>,
    access: PipeAccess,
    /// Stable writer-generation snapshot for the Linux named-FIFO exception
    /// that suppresses HUP on a nonblocking reader opened before any writer.
    /// `PipeInner::tx_generation` remains the truth source; this snapshot may
    /// become stale and only gates that endpoint's poll projection.
    initial_no_writer_generation: Option<u64>,
}

impl Drop for PipeEndpoint {
    fn drop(&mut self) {
        retire_participation(&self.pipe, self.access);
    }
}

struct PipeAdmission {
    pipe: Option<Arc<Pipe>>,
    access: PipeAccess,
    observed: PartnerGeneration,
    initial_no_writer_generation: Option<u64>,
}

struct ParticipationStart {
    observed: PartnerGeneration,
    partner_present: bool,
}

impl PipeAdmission {
    fn begin(pipe: Arc<Pipe>, access: PipeAccess) -> Self {
        let start = add_participation(&pipe, access);
        Self {
            pipe: Some(pipe),
            access,
            observed: start.observed,
            initial_no_writer_generation: None,
        }
    }

    fn begin_nonblocking_reader(pipe: Arc<Pipe>) -> Self {
        let start = add_participation(&pipe, PipeAccess::Read);
        Self {
            pipe: Some(pipe),
            access: PipeAccess::Read,
            observed: start.observed,
            initial_no_writer_generation: (!start.partner_present).then_some(start.observed.tx),
        }
    }

    fn begin_nonblocking_writer(pipe: Arc<Pipe>) -> Result<Self, SysError> {
        let Some(start) = add_participation_if(&pipe, PipeAccess::Write, |inner| inner.rx_cnt > 0)
        else {
            return Err(SysError::NoSuchDeviceOrAddress);
        };
        Ok(Self {
            pipe: Some(pipe),
            access: PipeAccess::Write,
            observed: start.observed,
            initial_no_writer_generation: None,
        })
    }

    fn partner_arrived(&self) -> bool {
        self.pipe
            .as_ref()
            .expect("active pipe admission lost its Pipe")
            .partner_arrived(self.access, self.observed)
    }

    fn wait_for_partner(&self) -> bool {
        self.pipe
            .as_ref()
            .expect("active pipe admission lost its Pipe")
            .wait_for_partner(self.access, self.observed)
    }

    fn commit(mut self) -> PipeEndpoint {
        let pipe = self
            .pipe
            .take()
            .expect("pipe admission committed more than once");
        PipeEndpoint {
            pipe,
            access: self.access,
            initial_no_writer_generation: self.initial_no_writer_generation,
        }
    }
}

impl Drop for PipeAdmission {
    fn drop(&mut self) {
        if let Some(pipe) = self.pipe.take() {
            retire_participation(&pipe, self.access);
        }
    }
}

fn add_participation(pipe: &Arc<Pipe>, access: PipeAccess) -> ParticipationStart {
    add_participation_if(pipe, access, |_| true)
        .expect("unconditional pipe participation was rejected")
}

fn add_participation_if(
    pipe: &Arc<Pipe>,
    access: PipeAccess,
    admit: impl FnOnce(&PipeInner) -> bool,
) -> Option<ParticipationStart> {
    let (start, rx_routes, tx_routes) = {
        let mut inner = pipe.inner.lock();
        if !admit(&inner) {
            return None;
        }
        let observed = PartnerGeneration {
            rx: inner.rx_generation,
            tx: inner.tx_generation,
        };
        let partner_present = match access {
            PipeAccess::Read => inner.tx_cnt > 0,
            PipeAccess::Write => inner.rx_cnt > 0,
            PipeAccess::ReadWrite => true,
        };
        let mut rx_routes = None;
        let mut tx_routes = None;
        if access.can_read() {
            let was_empty = inner.rx_cnt == 0;
            inner.rx_cnt = inner
                .rx_cnt
                .checked_add(1)
                .expect("pipe reader participant count overflow");
            inner.rx_generation = inner
                .rx_generation
                .checked_add(1)
                .expect("pipe reader admission generation overflow");
            if was_empty {
                tx_routes = Some(inner.tx_poll_routes.clone());
            }
        }
        if access.can_write() {
            let was_empty = inner.tx_cnt == 0;
            inner.tx_cnt = inner
                .tx_cnt
                .checked_add(1)
                .expect("pipe writer participant count overflow");
            inner.tx_generation = inner
                .tx_generation
                .checked_add(1)
                .expect("pipe writer admission generation overflow");
            if was_empty {
                rx_routes = Some(inner.rx_poll_routes.clone());
            }
        }
        (
            ParticipationStart {
                observed,
                partner_present,
            },
            rx_routes,
            tx_routes,
        )
    };

    if access.can_read() {
        pipe.write_recheck.publish(usize::MAX, false);
    }
    if access.can_write() {
        pipe.read_recheck.publish(usize::MAX, false);
    }
    notify_pipe_poll_routes(tx_routes, None, "tx", "reader_join");
    notify_pipe_poll_routes(rx_routes, None, "rx", "writer_join");
    Some(start)
}

fn retire_participation(pipe: &Arc<Pipe>, access: PipeAccess) {
    let (rx_routes, tx_routes) = {
        let mut inner = pipe.inner.lock();
        let mut rx_routes = None;
        let mut tx_routes = None;
        if access.can_read() {
            assert!(inner.rx_cnt > 0, "pipe reader participant count underflow");
            inner.rx_cnt -= 1;
            if inner.rx_cnt == 0 {
                tx_routes = Some(inner.tx_poll_routes.clone());
            }
        }
        if access.can_write() {
            assert!(inner.tx_cnt > 0, "pipe writer participant count underflow");
            inner.tx_cnt -= 1;
            if inner.tx_cnt == 0 {
                rx_routes = Some(inner.rx_poll_routes.clone());
            }
        }
        (rx_routes, tx_routes)
    };

    if access.can_read() {
        pipe.write_recheck.publish(usize::MAX, false);
    }
    if access.can_write() {
        pipe.read_recheck.publish(usize::MAX, false);
    }
    notify_pipe_poll_routes(tx_routes, None, "tx", "reader_retire");
    notify_pipe_poll_routes(rx_routes, None, "rx", "writer_retire");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipeEndpointInfo {
    access: PipeAccess,
}

impl PipeEndpointInfo {
    pub const fn can_read(self) -> bool {
        self.access.can_read()
    }

    pub const fn can_write(self) -> bool {
        self.access.can_write()
    }
}

pub fn pipe_endpoint_info(file: &File) -> Option<PipeEndpointInfo> {
    file.prv()
        .cast::<PipeEndpoint>()
        .map(|endpoint| PipeEndpointInfo {
            access: endpoint.access,
        })
}

pub fn pipe_endpoints_same_pipe(lhs: &File, rhs: &File) -> Result<bool, SysError> {
    let lhs = pipe_state(lhs).ok_or(SysError::InvalidArgument)?;
    let rhs = pipe_state(rhs).ok_or(SysError::InvalidArgument)?;

    // Equality is a one-shot owner-side behavior check for splice/tee errno
    // routing. The syscall layer must not receive or cache the pipe object or a
    // derived pipe id as protocol state.
    Ok(Arc::ptr_eq(lhs, rhs))
}

fn pipe_endpoint(file: &File) -> Option<&PipeEndpoint> {
    file.prv().cast::<PipeEndpoint>()
}

fn pipe_state(file: &File) -> Option<&Arc<Pipe>> {
    pipe_endpoint(file).map(|endpoint| &endpoint.pipe)
}

pub(super) fn display_name(file: &File) -> Option<PathBuf> {
    pipe_endpoint(file).map(|_| {
        let target = format!("pipe:[{}]", file.inode().ino().get());
        PathBuf::from(target.as_str())
    })
}

static PIPE_FILE_OPS: FileOps = FileOps {
    read: io::pipe_rx_read,
    write: io::pipe_tx_write,
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: poll::pipe_poll,
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
    inode.inode().inode().fifo_anchor().install(&rx.pipe);

    let rx = anony_open_with(
        &inode,
        OpenedFile::with_mode(&PIPE_FILE_OPS, FileMode::STREAM, AnyOpaque::new(rx)),
    )?;
    let tx = anony_open_with(
        &inode,
        OpenedFile::with_mode(&PIPE_FILE_OPS, FileMode::STREAM, AnyOpaque::new(tx)),
    )?;

    Ok(OpenedPipe { rx, tx })
}

pub(in crate::fs) fn open_named_fifo(
    path: PathRef,
    context: FifoOpenContext,
) -> Result<File, SysError> {
    assert_eq!(
        path.inode().ty(),
        InodeType::Fifo,
        "named FIFO activation received a non-FIFO path"
    );
    let pipe = path.inode().inode().fifo_anchor().get_or_create()?;
    let admission = if context.access == FifoOpenAccess::Read && context.nonblock {
        PipeAdmission::begin_nonblocking_reader(pipe)
    } else if context.access == FifoOpenAccess::Write && context.nonblock {
        // Reader observation and writer participation are one PipeInner
        // transaction. A writer that returns ENXIO must never advance a
        // generation or wake a concurrent blocking reader as a phantom peer.
        PipeAdmission::begin_nonblocking_writer(pipe)?
    } else {
        PipeAdmission::begin(pipe, context.access.into())
    };

    match context.access {
        FifoOpenAccess::Read if context.nonblock => {},
        FifoOpenAccess::Write if context.nonblock => {},
        FifoOpenAccess::Read | FifoOpenAccess::Write => {
            if !admission.wait_for_partner() {
                // Blocking FIFO open has not published an fd and the admission
                // guard rolls this attempt back exactly, so the whole syscall
                // is safe for the existing idempotent restart mechanism. A
                // restarted admission is a new partner round; no generation
                // from the canceled round is carried across the signal frame.
                return Err(SysError::RestartSyscall(RestartSyscall::Idempotent));
            }
        },
        FifoOpenAccess::ReadWrite => {},
    }

    let endpoint = admission.commit();
    Ok(File::new_with_mode(
        path,
        &PIPE_FILE_OPS,
        FileMode::STREAM,
        AnyOpaque::new(endpoint),
    ))
}

pub(in crate::fs) fn validate_fifo_open_status(_status: FileOpStatusFlags) -> Result<(), SysError> {
    // All status bits reaching this point were normalized and accepted by the
    // open ABI parser. Pipe I/O reads the opened-description snapshot on each
    // operation, so validation neither caches flags nor touches session state.
    Ok(())
}

#[cfg(feature = "kunit")]
mod kunits {
    use alloc::{vec, vec::Vec};

    use super::*;

    #[kunit]
    fn fifo_anchor_reuses_live_session_and_replaces_stale_session() {
        let anchor = FifoAnchor::new();
        let first = anchor.get_or_create().unwrap();
        let same = anchor.get_or_create().unwrap();
        assert!(Arc::ptr_eq(&first, &same));

        let stale = Arc::downgrade(&first);
        drop(same);
        drop(first);
        assert!(stale.upgrade().is_none());

        let fresh = anchor.get_or_create().unwrap();
        assert_eq!(fresh.inner.lock().capacity(), PIPE_DEFAULT_CAPACITY_BYTES);
        assert_eq!(fresh.inner.lock().rx_cnt, 0);
        assert_eq!(fresh.inner.lock().tx_cnt, 0);
    }

    #[kunit]
    fn pending_partner_generation_survives_short_overlap_and_cancel_rolls_back() {
        let pipe = Pipe::try_new().unwrap();
        let reader = PipeAdmission::begin(pipe.clone(), PipeAccess::Read);
        assert!(!reader.partner_arrived());

        let writer = PipeAdmission::begin(pipe.clone(), PipeAccess::Write);
        assert!(writer.partner_arrived());
        assert!(reader.partner_arrived());
        drop(writer);

        let inner = pipe.inner.lock();
        assert_eq!(inner.rx_cnt, 1);
        assert_eq!(inner.tx_cnt, 0);
        drop(inner);
        assert!(
            reader.partner_arrived(),
            "overlapping writer generation must commit the waiting reader"
        );

        drop(reader);
        let inner = pipe.inner.lock();
        assert_eq!(inner.rx_cnt, 0);
        assert_eq!(inner.tx_cnt, 0);
    }

    #[kunit]
    fn rejected_nonblocking_writer_never_publishes_a_partner_generation() {
        let pipe = Pipe::try_new().unwrap();
        assert!(PipeAdmission::begin_nonblocking_writer(pipe.clone()).is_err());
        {
            let inner = pipe.inner.lock();
            assert_eq!(inner.rx_cnt, 0);
            assert_eq!(inner.tx_cnt, 0);
            assert_eq!(inner.rx_generation, 0);
            assert_eq!(inner.tx_generation, 0);
        }

        let reader = PipeAdmission::begin(pipe, PipeAccess::Read);
        assert!(!reader.partner_arrived());
    }

    #[kunit]
    fn initial_nonblocking_reader_suppresses_hup_until_a_writer_generation() {
        let pipe = Pipe::try_new().unwrap();
        let reader = PipeAdmission::begin_nonblocking_reader(pipe.clone()).commit();
        assert_eq!(reader.initial_no_writer_generation, Some(0));

        let writer = PipeAdmission::begin(pipe.clone(), PipeAccess::Write).commit();
        drop(writer);
        assert_ne!(
            pipe.inner.lock().tx_generation,
            reader.initial_no_writer_generation.unwrap()
        );
    }

    #[kunit]
    fn duplex_endpoint_contributes_and_retires_each_side_once() {
        let pipe = Pipe::try_new().unwrap();
        let endpoint = PipeAdmission::begin(pipe.clone(), PipeAccess::ReadWrite).commit();
        {
            let inner = pipe.inner.lock();
            assert_eq!(inner.rx_cnt, 1);
            assert_eq!(inner.tx_cnt, 1);
            assert_eq!(inner.rx_generation, 1);
            assert_eq!(inner.tx_generation, 1);
        }

        drop(endpoint);
        let inner = pipe.inner.lock();
        assert_eq!(inner.rx_cnt, 0);
        assert_eq!(inner.tx_cnt, 0);
    }

    #[kunit]
    fn distinct_fifo_anchors_never_share_runtime_session() {
        let first = FifoAnchor::new().get_or_create().unwrap();
        let second = FifoAnchor::new().get_or_create().unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
    }

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
