use crate::{
    fs::{FileMode, PollEvent, PollRegisterResult, PollRequest},
    prelude::*,
    task::files::{FileDesc, FileDescOps, OpenedFileFinalReleaseCtx},
    utils::any_opaque::AnyOpaque,
};

use super::{
    TtyEndpoint, TtyProgress, TtyWakeHandle,
    discipline::{InputRead, TtySignalControl},
    file::{self, TtyFile, TtyOperation},
    port::{TtyLineSnapshot, TtyRxUnit},
    relation::{self, RelationEnrollment},
    terminal::Terminal,
};

const DESCRIPTION_PREPARED: u8 = 0;
const DESCRIPTION_LIVE: u8 = 1;
const DESCRIPTION_RELEASED: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PairPhase {
    Prepared,
    Live,
    Retired,
}

struct PairInner {
    phase: PairPhase,
    slave_descriptions: usize,
}

/// Sole owner of one PTY episode's liveness and peer participation.
///
/// `operation` only orders a bounded Terminal commit against description
/// release. It is not state truth and is never held while an operation waits.
pub(crate) struct PtyPairState {
    terminal: Arc<Terminal>,
    operation: Mutex<()>,
    inner: SpinLock<PairInner>,
}

impl PtyPairState {
    fn try_new(terminal: Arc<Terminal>) -> Result<Arc<Self>, SysError> {
        Arc::try_new(Self {
            terminal,
            operation: Mutex::new(()),
            inner: SpinLock::new(PairInner {
                phase: PairPhase::Prepared,
                slave_descriptions: 0,
            }),
        })
        .map_err(|_| SysError::OutOfMemory)
    }

    fn commit_master(&self, description_phase: &AtomicU8) {
        let _operation = self.operation.lock();
        let mut inner = self.inner.lock();
        assert_eq!(
            inner.phase,
            PairPhase::Prepared,
            "PTY master description committed outside pair prepare"
        );
        assert_eq!(
            description_phase.compare_exchange(
                DESCRIPTION_PREPARED,
                DESCRIPTION_LIVE,
                Ordering::AcqRel,
                Ordering::Acquire,
            ),
            Ok(DESCRIPTION_PREPARED),
            "PTY master description committed more than once"
        );
        inner.phase = PairPhase::Live;
    }

    fn commit_slave(&self, description_phase: &AtomicU8) -> Result<(), SysError> {
        {
            let _operation = self.operation.lock();
            let mut inner = self.inner.lock();
            if inner.phase != PairPhase::Live {
                return Err(SysError::IO);
            }
            assert_eq!(
                description_phase.compare_exchange(
                    DESCRIPTION_PREPARED,
                    DESCRIPTION_LIVE,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ),
                Ok(DESCRIPTION_PREPARED),
                "PTY slave description committed more than once"
            );
            inner.slave_descriptions = inner
                .slave_descriptions
                .checked_add(1)
                .expect("PTY slave description count overflow");
        }
        Ok(())
    }

    fn release_slave(&self, description_phase: &AtomicU8) {
        {
            let _operation = self.operation.lock();
            if description_phase.swap(DESCRIPTION_RELEASED, Ordering::AcqRel) != DESCRIPTION_LIVE {
                return;
            }
            let mut inner = self.inner.lock();
            assert!(
                inner.slave_descriptions != 0,
                "PTY slave description count underflow"
            );
            inner.slave_descriptions -= 1;
            let peer_became_absent =
                inner.phase == PairPhase::Live && inner.slave_descriptions == 0;
            drop(inner);
            // Linux discards the now-unconsumable slave input at the last slave
            // close, while master-readable slave output keeps buffered precedence.
            // Keep a reopening description behind the input flush so the old
            // last-close episode cannot discard bytes admitted for the new peer.
            if peer_became_absent {
                self.terminal.pty_peer_absent();
            }
        }
        self.notify_state_change();
    }

    fn retire_master(&self, description_phase: &AtomicU8) {
        {
            let _operation = self.operation.lock();
            if description_phase.swap(DESCRIPTION_RELEASED, Ordering::AcqRel) != DESCRIPTION_LIVE {
                return;
            }
            let mut inner = self.inner.lock();
            assert_eq!(
                inner.phase,
                PairPhase::Live,
                "PTY master description released outside live pair"
            );
            inner.phase = PairPhase::Retired;
        }
        // Pair retirement is already irreversible before Terminal cleanup.
        // Flush and wake happen with no pair guard held.
        self.terminal.pty_hangup();
        self.notify_state_change();
    }

    fn live_slave_count(&self) -> Option<usize> {
        let inner = self.inner.lock();
        (inner.phase == PairPhase::Live).then_some(inner.slave_descriptions)
    }

    fn is_retired(&self) -> bool {
        self.inner.lock().phase == PairPhase::Retired
    }

    fn wait_until(&self, predicate: impl Fn() -> bool) -> Result<(), SysError> {
        self.terminal.wait_for_progress(predicate)
    }

    fn register_poll_route(&self, request: &PollRequest<'_>) -> bool {
        self.terminal.register_progress_route(request)
    }

    fn notify_state_change(&self) {
        if self.terminal.drain_check_pending() && !self.terminal.output_pending() {
            self.terminal.complete_drain_if(true);
        }
        // Pair predicates share the Terminal's existing progress channel. The
        // route registry is notification storage only; readiness and lifecycle
        // truth remain in Terminal and pair respectively.
        self.terminal.publish_progress();
    }
}

impl TtyProgress for PtyPairState {
    fn wake(&self) {
        self.notify_state_change();
    }
}

struct PtyMasterDescription {
    pair: Arc<PtyPairState>,
    phase: AtomicU8,
    base_final_release: Option<for<'a> fn(OpenedFileFinalReleaseCtx<'a>)>,
}

impl PtyMasterDescription {
    fn is_live(&self) -> bool {
        self.phase.load(Ordering::Acquire) == DESCRIPTION_LIVE && !self.pair.is_retired()
    }

    fn release(&self) {
        self.pair.retire_master(&self.phase);
    }
}

impl TtyOperation for PtyMasterDescription {
    fn run_if_live(&self, operation: &mut dyn FnMut()) -> Result<(), SysError> {
        let _operation = self.pair.operation.lock();
        if !self.is_live() {
            return Err(SysError::IO);
        }
        operation();
        Ok(())
    }
}

#[derive(Opaque)]
struct PtyMasterFile {
    terminal_file: TtyFile,
    description: Arc<PtyMasterDescription>,
}

fn master_file(file: &File) -> &PtyMasterFile {
    file.private::<PtyMasterFile>()
        .expect("PTY master FileOps received non-master private state")
}

pub(crate) struct PreparedPtyPair {
    pair: Arc<PtyPairState>,
    endpoint: Arc<TtyEndpoint>,
    enrollment: RelationEnrollment,
    master_description: Arc<PtyMasterDescription>,
    opened_master: Option<OpenedFile>,
    description_ops: FileDescOps,
}

pub(crate) fn prepare_pair(
    line: TtyLineSnapshot,
    mut base_description_ops: FileDescOps,
) -> Result<PreparedPtyPair, SysError> {
    let terminal = Terminal::try_new(line)?;
    let pair = PtyPairState::try_new(terminal.clone())?;
    let progress: Arc<dyn TtyProgress> = pair.clone();
    let endpoint = Arc::try_new(TtyEndpoint {
        terminal,
        wake_source: Arc::downgrade(&progress),
    })
    .map_err(|_| SysError::OutOfMemory)?;
    let enrollment = RelationEnrollment::new(endpoint.clone());
    let master_description = Arc::try_new(PtyMasterDescription {
        pair: pair.clone(),
        phase: AtomicU8::new(DESCRIPTION_PREPARED),
        base_final_release: base_description_ops.final_release,
    })
    .map_err(|_| SysError::OutOfMemory)?;
    base_description_ops.final_release = Some(master_final_release);
    let opened_master = OpenedFile::with_mode(
        &PTY_MASTER_FILE_OPS,
        FileMode::STREAM,
        AnyOpaque::new(PtyMasterFile {
            terminal_file: file::terminal_file(
                endpoint.clone(),
                TtyWakeHandle { source: progress },
            ),
            description: master_description.clone(),
        }),
    );
    Ok(PreparedPtyPair {
        pair,
        endpoint,
        enrollment,
        master_description,
        opened_master: Some(opened_master),
        description_ops: base_description_ops,
    })
}

impl PreparedPtyPair {
    pub(crate) fn take_opened_master(&mut self) -> OpenedFile {
        self.opened_master
            .take()
            .expect("PTY master opened file consumed more than once")
    }

    pub(crate) fn description_ops(&self) -> FileDescOps {
        self.description_ops
    }

    pub(crate) fn commit(
        self,
        prepared_master: Arc<FileDesc>,
        success_tail: impl FnOnce(&LivePtyPair, Arc<FileDesc>),
    ) -> LivePtyPair {
        assert!(
            self.opened_master.is_none(),
            "PTY pair committed before master description prepare completed"
        );
        assert!(
            Arc::ptr_eq(
                &master_file(prepared_master.vfs_file()).description,
                &self.master_description,
            ),
            "PTY pair committed with a different master opened description"
        );
        self.pair.commit_master(&self.master_description.phase);
        let live = LivePtyPair {
            pair: self.pair,
            endpoint: self.endpoint,
            _enrollment: self.enrollment,
        };
        // Pair commit is the first infallible success-tail step. Keep fd/devpts
        // publication in the caller's owner, but do not return a live pair until
        // that static tail has consumed the exact prepared description.
        success_tail(&live, prepared_master);
        live
    }
}

pub(crate) struct LivePtyPair {
    pair: Arc<PtyPairState>,
    endpoint: Arc<TtyEndpoint>,
    /// Stage 2 deliberately keeps this pre-visibility authority uncommitted.
    /// Stage 3 will move it into the allocation transaction success tail.
    _enrollment: RelationEnrollment,
}

impl LivePtyPair {
    pub(crate) fn prepare_slave_description(
        &self,
        mut base_description_ops: FileDescOps,
    ) -> Result<PreparedPtySlaveDescription, SysError> {
        if self.pair.live_slave_count().is_none() {
            return Err(SysError::IO);
        }
        let description = Arc::try_new(PtySlaveDescription {
            pair: self.pair.clone(),
            phase: AtomicU8::new(DESCRIPTION_PREPARED),
            base_final_release: base_description_ops.final_release,
        })
        .map_err(|_| SysError::OutOfMemory)?;
        let progress: Arc<dyn TtyProgress> = self.pair.clone();
        let opened = file::opened_pty_slave_file(
            self.endpoint.clone(),
            TtyWakeHandle { source: progress },
            description.clone(),
        );
        base_description_ops.final_release = Some(slave_final_release);
        Ok(PreparedPtySlaveDescription {
            description,
            opened: Some(opened),
            description_ops: base_description_ops,
        })
    }
}

pub(crate) struct PreparedPtySlaveDescription {
    description: Arc<PtySlaveDescription>,
    opened: Option<OpenedFile>,
    description_ops: FileDescOps,
}

impl PreparedPtySlaveDescription {
    pub(crate) fn take_opened_file(&mut self) -> OpenedFile {
        self.opened
            .take()
            .expect("PTY slave opened file consumed more than once")
    }

    pub(crate) fn description_ops(&self) -> FileDescOps {
        self.description_ops
    }

    pub(crate) fn commit(
        self,
        prepared_slave: Arc<FileDesc>,
        success_tail: impl FnOnce(Arc<FileDesc>),
    ) -> Result<(), SysError> {
        assert!(
            self.opened.is_none(),
            "PTY slave participation committed before description prepare completed"
        );
        assert!(
            core::ptr::eq(
                file::pty_slave_description(prepared_slave.vfs_file()),
                self.description.as_ref(),
            ),
            "PTY slave participation committed with a different opened description"
        );
        self.description
            .pair
            .commit_slave(&self.description.phase)?;
        // The future route owner may compose relation and fd publication here,
        // but it cannot return between participation and that infallible tail.
        success_tail(prepared_slave);
        self.description.pair.notify_state_change();
        Ok(())
    }
}

pub(super) struct PtySlaveDescription {
    pair: Arc<PtyPairState>,
    phase: AtomicU8,
    base_final_release: Option<for<'a> fn(OpenedFileFinalReleaseCtx<'a>)>,
}

impl PtySlaveDescription {
    fn phase_is_live(&self) -> bool {
        self.phase.load(Ordering::Acquire) == DESCRIPTION_LIVE
    }

    pub(super) fn is_released_or_hung_up(&self) -> bool {
        !self.phase_is_live() || self.pair.is_retired()
    }

    pub(super) fn read(
        &self,
        terminal: &Terminal,
        dst: &mut [u8],
        ctx: FileIoCtx,
    ) -> Result<usize, SysError> {
        loop {
            let result = {
                let _operation = self.pair.operation.lock();
                if !self.phase_is_live() || self.pair.is_retired() {
                    return Ok(0);
                }
                terminal.read_pty_input(dst)
            };
            match result {
                InputRead::Bytes(count) => {
                    self.pair.notify_state_change();
                    return Ok(count);
                },
                InputRead::Eof => {
                    self.pair.notify_state_change();
                    return Ok(0);
                },
                InputRead::Empty if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) => {
                    return Err(SysError::Again);
                },
                InputRead::Empty => self
                    .pair
                    .wait_until(|| self.is_released_or_hung_up() || terminal.readable())?,
            }
        }
    }

    pub(super) fn write(
        &self,
        terminal: &Terminal,
        source: &[u8],
        ctx: FileIoCtx,
    ) -> Result<usize, SysError> {
        loop {
            let written = {
                let _operation = self.pair.operation.lock();
                if !self.phase_is_live() || self.pair.is_retired() {
                    return Err(SysError::IO);
                }
                terminal.enqueue_pty_output(source)
            };
            if written != 0 {
                self.pair.notify_state_change();
                return Ok(written);
            }
            if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
                return Err(SysError::Again);
            }
            self.pair
                .wait_until(|| self.is_released_or_hung_up() || terminal.writable())?;
        }
    }

    pub(super) fn poll(
        &self,
        terminal: &Terminal,
        request: &PollRequest<'_>,
    ) -> PollRegisterResult {
        if request.is_register() {
            if !self.pair.register_poll_route(request) {
                return PollRegisterResult::Unsupported;
            }
            // Registration runs inside an active scheduler wait and cannot take
            // the sleepable operation mutex. The route is installed first, so
            // any lifecycle/data-plane transition crossing these owner snapshots
            // pretriggers the round and forces a consistent final snapshot.
            return PollRegisterResult::Subscribed(self.poll_events(terminal, request.interests()));
        }

        // Snapshot scans run outside an active wait. Serialize pair lifecycle
        // with Terminal cleanup so the result cannot combine pre-retirement
        // liveness with post-retirement flushed buffers.
        let _operation = self.pair.operation.lock();
        PollRegisterResult::Ready(self.poll_events(terminal, request.interests()))
    }

    fn poll_events(&self, terminal: &Terminal, interests: PollEvent) -> PollEvent {
        if self.is_released_or_hung_up() {
            (interests & (PollEvent::READABLE | PollEvent::WRITABLE))
                | PollEvent::ERROR
                | PollEvent::HANG_UP
        } else {
            terminal
                .poll(&PollRequest::snapshot(interests))
                .expect_ready("PTY slave Terminal snapshot")
        }
    }

    fn release(&self) {
        self.pair.release_slave(&self.phase);
    }
}

impl TtyOperation for PtySlaveDescription {
    fn run_if_live(&self, operation: &mut dyn FnMut()) -> Result<(), SysError> {
        let _operation = self.pair.operation.lock();
        if self.is_released_or_hung_up() {
            return Err(SysError::IO);
        }
        operation();
        Ok(())
    }
}

fn master_read(
    file: &File,
    _pos: &mut usize,
    dst: &mut [u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let master = master_file(file);
    loop {
        let outcome = {
            let _operation = master.description.pair.operation.lock();
            if !master.description.is_live() {
                return Err(SysError::IO);
            }
            let count = master.description.pair.terminal.read_output(dst);
            if count != 0 {
                Some(Ok(count))
            } else if master.description.pair.live_slave_count() == Some(0) {
                Some(Err(SysError::IO))
            } else {
                None
            }
        };
        if let Some(outcome) = outcome {
            master.description.pair.notify_state_change();
            return outcome;
        }
        if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
            return Err(SysError::Again);
        }
        master.description.pair.wait_until(|| {
            !master.description.is_live()
                || master.description.pair.terminal.output_pending()
                || master.description.pair.live_slave_count() == Some(0)
        })?;
    }
}

fn master_write(
    file: &File,
    _pos: &mut usize,
    source: &[u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let master = master_file(file);
    let mut written = 0;
    while written < source.len() {
        let effect = {
            let _operation = master.description.pair.operation.lock();
            if !master.description.is_live() {
                return if written == 0 {
                    Err(SysError::IO)
                } else {
                    Ok(written)
                };
            }
            if master.description.pair.live_slave_count() == Some(0) {
                // Linux accepts master writes while the slave has no opened
                // description, but the bytes are not retained for a later
                // reopen. This is an intentional ABI result, not a success stub.
                return Ok(source.len());
            }
            master
                .description
                .pair
                .terminal
                .receive_pty_rx_unit_effect(TtyRxUnit::Byte(source[written]))
        };
        if effect.consumed() {
            written += 1;
            master.description.pair.notify_state_change();
            if let Some(signal) = effect.signal() {
                let signal = match signal {
                    TtySignalControl::Interrupt => {
                        crate::task::jobctl::TtyTerminalSignal::Interrupt
                    },
                    TtySignalControl::Quit => crate::task::jobctl::TtyTerminalSignal::Quit,
                    TtySignalControl::Suspend => crate::task::jobctl::TtyTerminalSignal::Suspend,
                };
                if !relation::signal_foreground(&master.terminal_file.endpoint, signal) {
                    master
                        .description
                        .pair
                        .terminal
                        .record_no_foreground_input_signal();
                }
            }
            continue;
        }
        if written != 0 {
            return Ok(written);
        }
        if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
            return Err(SysError::Again);
        }
        let byte = source[written];
        master.description.pair.wait_until(|| {
            !master.description.is_live()
                || master.description.pair.live_slave_count() == Some(0)
                || master
                    .description
                    .pair
                    .terminal
                    .can_receive_rx_unit(TtyRxUnit::Byte(byte))
        })?;
    }
    Ok(written)
}

fn master_poll(file: &File, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
    let master = master_file(file);
    let interests = request.interests();
    if request.is_register() {
        if !master.description.pair.register_poll_route(request) {
            return Ok(PollRegisterResult::Unsupported);
        }
        // See the slave register path: the installed route makes a mixed owner
        // snapshot a recheck hint, never the final readiness decision.
        return Ok(PollRegisterResult::Subscribed(master_poll_events(
            master, interests,
        )));
    }

    // Final snapshots run after the iomux wait round is retired and may take
    // the operation mutex to serialize lifecycle with Terminal cleanup.
    let _operation = master.description.pair.operation.lock();
    Ok(PollRegisterResult::Ready(master_poll_events(
        master, interests,
    )))
}

fn master_poll_events(master: &PtyMasterFile, interests: PollEvent) -> PollEvent {
    let mut ready = PollEvent::empty();
    let peer = master.description.pair.live_slave_count();
    if !master.description.is_live() {
        ready |= PollEvent::ERROR | PollEvent::HANG_UP;
        ready |= interests & (PollEvent::READABLE | PollEvent::WRITABLE);
    } else {
        if interests.contains(PollEvent::READABLE)
            && master.description.pair.terminal.output_pending()
        {
            ready |= PollEvent::READABLE;
        }
        if interests.contains(PollEvent::WRITABLE)
            && (peer == Some(0) || master.description.pair.terminal.input_writable())
        {
            ready |= PollEvent::WRITABLE;
        }
        if peer == Some(0) {
            ready |= PollEvent::HANG_UP;
        }
    }
    ready
}

fn master_ioctl(file: &File, ctx: IoctlCtx<'_>) -> Result<u64, SysError> {
    let master = master_file(file);
    if !master.description.is_live() {
        return Err(SysError::IO);
    }
    // Master and slave observe the same Terminal truth, but the master never
    // forwards controlling-terminal operations to the relation owner.
    let result = file::terminal_ioctl(
        &master.terminal_file,
        false,
        Some(master.description.as_ref()),
        ctx,
    );
    result
}

fn master_final_release(ctx: OpenedFileFinalReleaseCtx<'_>) {
    let master = master_file(ctx.file);
    master.description.release();
    if let Some(base) = master.description.base_final_release {
        base(ctx);
    }
}

fn slave_final_release(ctx: OpenedFileFinalReleaseCtx<'_>) {
    let description = file::pty_slave_description(ctx.file);
    description.release();
    if let Some(base) = description.base_final_release {
        base(ctx);
    }
}

fn check_status_flags(_file: &File, flags: FileOpStatusFlags) -> Result<(), SysError> {
    if !(flags - FileOpStatusFlags::NONBLOCK).is_empty() {
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

static PTY_MASTER_FILE_OPS: FileOps = FileOps {
    read: master_read,
    write: master_write,
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: master_poll,
    fcntl: None,
    ioctl: master_ioctl,
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{device::console::open_console_stdin, fs::anony_open_with};

    fn line() -> TtyLineSnapshot {
        TtyLineSnapshot {
            baud: 115200,
            parity: super::super::port::TtyParity::None,
            data_bits: 8,
        }
    }

    fn materialize(opened: OpenedFile) -> File {
        let placeholder = open_console_stdin();
        anony_open_with(placeholder.path(), opened).unwrap()
    }

    fn live_pair() -> (LivePtyPair, Arc<PtyMasterDescription>, Arc<File>) {
        let mut prepared = prepare_pair(line(), FileDescOps::default()).unwrap();
        let master = Arc::new(materialize(prepared.take_opened_master()));
        let PreparedPtyPair {
            pair,
            endpoint,
            enrollment,
            master_description,
            opened_master,
            description_ops: _,
        } = prepared;
        assert!(opened_master.is_none());
        pair.commit_master(&master_description.phase);
        let live = LivePtyPair {
            pair,
            endpoint,
            _enrollment: enrollment,
        };
        (live, master_description, master)
    }

    fn live_slave(pair: &LivePtyPair) -> (Arc<PtySlaveDescription>, Arc<File>) {
        let mut prepared = pair
            .prepare_slave_description(FileDescOps::default())
            .unwrap();
        let slave = Arc::new(materialize(prepared.take_opened_file()));
        let PreparedPtySlaveDescription {
            description,
            opened,
            description_ops: _,
        } = prepared;
        assert!(opened.is_none());
        pair.pair.commit_slave(&description.phase).unwrap();
        (description, slave)
    }

    #[kunit]
    fn pair_data_plane_peer_absence_reopen_and_hangup_matrix() {
        let (pair, master_description, master) = live_pair();
        let interests = PollEvent::READABLE | PollEvent::WRITABLE;
        let nonblocking = FileIoCtx::new(FileOpStatusFlags::NONBLOCK);
        assert_eq!(
            master.poll(&PollRequest::snapshot(interests)).unwrap(),
            PollRegisterResult::Ready(PollEvent::WRITABLE | PollEvent::HANG_UP)
        );
        assert_eq!(master.write_with_ctx(b"discarded\n", nonblocking), Ok(10));

        let (slave_description, slave) = live_slave(&pair);
        assert_eq!(pair.pair.live_slave_count(), Some(1));
        assert_eq!(master.write_with_ctx(b"input\n", nonblocking), Ok(6));
        let mut input = [0_u8; 8];
        assert_eq!(slave.read_with_ctx(&mut input, nonblocking), Ok(6));
        assert_eq!(&input[..6], b"input\n");

        let mut output = [0_u8; 8];
        assert_eq!(master.read_with_ctx(&mut output, nonblocking), Ok(7));
        assert_eq!(&output[..7], b"input\r\n");

        assert_eq!(slave.write_with_ctx(b"out\n", nonblocking), Ok(4));
        assert_eq!(master.read_with_ctx(&mut output, nonblocking), Ok(5));
        assert_eq!(&output[..5], b"out\r\n");

        slave_description.release();
        assert_eq!(pair.pair.live_slave_count(), Some(0));
        assert_eq!(master.write_with_ctx(b"gone\n", nonblocking), Ok(5));
        assert_eq!(
            master.read_with_ctx(&mut output, nonblocking),
            Err(SysError::IO)
        );

        let (reopened_description, reopened) = live_slave(&pair);
        assert_eq!(
            reopened.read_with_ctx(&mut input, nonblocking),
            Err(SysError::Again)
        );
        master_description.release();
        assert!(pair.pair.is_retired());
        assert_eq!(reopened.read_with_ctx(&mut input, nonblocking), Ok(0));
        assert_eq!(
            reopened.write_with_ctx(b"x", nonblocking),
            Err(SysError::IO)
        );
        assert_eq!(
            reopened.poll(&PollRequest::snapshot(interests)).unwrap(),
            PollRegisterResult::Ready(
                PollEvent::READABLE | PollEvent::WRITABLE | PollEvent::ERROR | PollEvent::HANG_UP
            )
        );
        reopened_description.release();
    }
}
