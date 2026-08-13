use crate::{
    fs::{
        FileMode,
        fanotify::{FanHookEvent, FanMask, notify_path_event, observed_file_description_ops},
    },
    prelude::*,
    task::files::{
        FdFlags, FileDesc, FileDescOps, FileStatusFlags, LinuxOpenCompat, OpenAccessMode,
    },
    utils::any_opaque::AnyOpaque,
};

use super::{
    TtyEndpoint, TtyProgress, TtyWakeHandle, file as tty_file,
    relation::{self, RelationEnrollment},
    terminal::Terminal,
};

mod file;
mod pair;

use file::{
    PTY_MASTER_FILE_OPS, PtyMasterFile, master_file, master_final_release, slave_final_release,
};
pub(crate) use pair::PtyPairState;
use pair::{DESCRIPTION_PREPARED, PtyMasterDescription};
pub(super) use pair::{PtyEffectPermit, PtySlaveDescription};

pub(crate) struct PreparedPtyPair {
    pair: Arc<PtyPairState>,
    endpoint: Arc<TtyEndpoint>,
    enrollment: Option<RelationEnrollment>,
    master_description: Arc<PtyMasterDescription>,
    opened_master: Option<OpenedFile>,
}

pub(crate) fn prepare_pair(index: u32) -> Result<PreparedPtyPair, SysError> {
    let terminal = Terminal::try_new_pty()?;
    let pair = PtyPairState::try_new(terminal.clone())?;
    let progress: Arc<dyn TtyProgress> = pair.clone();
    let endpoint = Arc::try_new(TtyEndpoint {
        terminal,
        wake_source: Arc::downgrade(&progress),
    })
    .map_err(|_| SysError::OutOfMemory)?;
    let enrollment = RelationEnrollment::new(endpoint.clone());
    let master_description = Arc::try_new(PtyMasterDescription::new(pair.clone(), index))
        .map_err(|_| SysError::OutOfMemory)?;
    let opened_master = OpenedFile::with_mode(
        &PTY_MASTER_FILE_OPS,
        FileMode::STREAM,
        AnyOpaque::new(PtyMasterFile {
            terminal_file: tty_file::terminal_file(
                endpoint.clone(),
                TtyWakeHandle { source: progress },
            ),
            description: master_description.clone(),
        }),
    );
    Ok(PreparedPtyPair {
        pair,
        endpoint,
        enrollment: Some(enrollment),
        master_description,
        opened_master: Some(opened_master),
    })
}

impl PreparedPtyPair {
    pub(crate) fn pair_handle(&self) -> LivePtyPair {
        LivePtyPair {
            pair: self.pair.clone(),
            endpoint: self.endpoint.clone(),
        }
    }

    pub(crate) fn install_cleanup(
        &mut self,
        binding: PtyBindingCapability,
    ) -> Result<(), SysError> {
        let enrollment = self
            .enrollment
            .take()
            .expect("PTY relation enrollment consumed more than once");
        let participant = enrollment.commit()?;
        self.master_description
            .install_cleanup(binding, participant);
        Ok(())
    }

    pub(crate) fn abort_installed_cleanup(&mut self) {
        self.master_description.abort_prepared_cleanup();
    }

    pub(crate) fn take_opened_master(&mut self) -> OpenedFile {
        self.opened_master
            .take()
            .expect("PTY master opened file consumed more than once")
    }

    pub(crate) fn compose_description_ops(
        &mut self,
        mut base_description_ops: FileDescOps,
    ) -> FileDescOps {
        let mut installed = self.master_description.base_final_release.lock();
        assert!(
            installed.is_none(),
            "PTY master description hooks composed more than once"
        );
        *installed = Some(base_description_ops.final_release);
        drop(installed);
        base_description_ops.final_release = Some(master_final_release);
        base_description_ops
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
            self.enrollment.is_none(),
            "PTY pair committed before relation enrollment"
        );
        assert!(
            self.master_description.base_final_release.lock().is_some(),
            "PTY pair committed before static final-release composition"
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
        };
        // Pair commit is the first infallible success-tail step. Keep fd/devpts
        // publication in the caller's owner, but do not return a live pair until
        // that static tail has consumed the exact prepared description.
        success_tail(&live, prepared_master);
        live
    }
}

#[derive(Clone)]
pub(crate) struct LivePtyPair {
    pair: Arc<PtyPairState>,
    endpoint: Arc<TtyEndpoint>,
}

impl LivePtyPair {
    pub(crate) fn prepare_slave_description(
        &self,
    ) -> Result<PreparedPtySlaveDescription, SysError> {
        if self.pair.live_slave_count().is_none() || self.pair.slave_locked() {
            return Err(SysError::IO);
        }
        let description = Arc::try_new(PtySlaveDescription {
            pair: self.pair.clone(),
            phase: AtomicU8::new(DESCRIPTION_PREPARED),
            base_final_release: SpinLock::new(None),
        })
        .map_err(|_| SysError::OutOfMemory)?;
        let progress: Arc<dyn TtyProgress> = self.pair.clone();
        let opened = tty_file::opened_pty_slave_file(
            self.endpoint.clone(),
            TtyWakeHandle { source: progress },
            description.clone(),
        );
        Ok(PreparedPtySlaveDescription {
            description,
            opened: Some(opened),
        })
    }

    pub(crate) fn prepare_implicit_acquire(
        &self,
        readable: bool,
        no_ctty: bool,
    ) -> PtyImplicitAcquire {
        PtyImplicitAcquire(relation::prepare_implicit_acquire(
            self.endpoint.clone(),
            readable,
            no_ctty,
        ))
    }

    #[cfg(feature = "kunit")]
    pub(crate) fn live_slave_count(&self) -> Option<usize> {
        self.pair.live_slave_count()
    }

    #[cfg(feature = "kunit")]
    pub(crate) fn unlock_slave(&self) {
        self.pair.set_slave_locked(false).unwrap();
    }
}

pub(crate) struct PtyImplicitAcquire(relation::ImplicitAcquire);

impl PtyImplicitAcquire {
    pub(crate) fn commit(self) {
        self.0.commit();
    }
}

fn parse_peer_open_flags(
    flags: u32,
) -> Result<(OpenAccessMode, FileStatusFlags, FdFlags, bool), SysError> {
    use anemone_abi::fs::linux::open::*;

    const ACCEPTED: u32 = O_ACCMODE | O_NOCTTY | O_NONBLOCK | O_CLOEXEC;
    if flags & !ACCEPTED != 0 {
        return Err(SysError::InvalidArgument);
    }
    let access = match flags & O_ACCMODE {
        O_RDONLY => OpenAccessMode::Read,
        O_WRONLY => OpenAccessMode::Write,
        O_RDWR => OpenAccessMode::ReadWrite,
        _ => return Err(SysError::InvalidArgument),
    };
    let mut status = FileStatusFlags::empty();
    status.set(FileStatusFlags::NONBLOCK, flags & O_NONBLOCK != 0);
    Ok((
        access,
        status,
        FdFlags::from_linux_open_flags(flags),
        flags & O_NOCTTY != 0,
    ))
}

fn open_peer(
    master: &PtyMasterDescription,
    endpoint: Arc<TtyEndpoint>,
    ctx: &IoctlCtx<'_>,
) -> Result<u64, SysError> {
    let flags = u32::try_from(ctx.arg()).map_err(|_| SysError::InvalidArgument)?;
    let (access, status, fd_flags, no_ctty) = parse_peer_open_flags(flags)?;
    if !master.is_live() {
        return Err(SysError::IO);
    }
    let binding = master.binding().ok_or(SysError::IO)?;
    let reservation = ctx.reserve_fd()?;
    let path = binding.peer_path()?;
    let pair = LivePtyPair {
        pair: master.pair(),
        endpoint,
    };
    let mut prepared = pair.prepare_slave_description()?;
    let opened = prepared.take_opened_file();
    let description_ops = prepared.compose_description_ops(observed_file_description_ops());
    let file = opened.into_file(path.clone());
    file.check_status_flags(status.to_file_op_status_flags())?;
    let file_desc = FileDesc::new_opened(
        file,
        access,
        status,
        LinuxOpenCompat::empty(),
        fd_flags,
        description_ops,
    );
    let relation = pair.prepare_implicit_acquire(access.can_read(), no_ctty);
    let reserved_fd = reservation.fd();
    prepared.commit(file_desc, |description| {
        relation.commit();
        notify_path_event(FanHookEvent::new(FanMask::OPEN, path));
        let committed = reservation.commit(description);
        assert_eq!(committed, reserved_fd, "PTY peer fd reservation changed");
    })?;
    Ok(reserved_fd.raw() as u64)
}

pub(crate) struct PreparedPtySlaveDescription {
    description: Arc<PtySlaveDescription>,
    opened: Option<OpenedFile>,
}

impl PreparedPtySlaveDescription {
    pub(crate) fn take_opened_file(&mut self) -> OpenedFile {
        self.opened
            .take()
            .expect("PTY slave opened file consumed more than once")
    }

    pub(crate) fn compose_description_ops(
        &mut self,
        mut base_description_ops: FileDescOps,
    ) -> FileDescOps {
        let mut installed = self.description.base_final_release.lock();
        assert!(
            installed.is_none(),
            "PTY slave description hooks composed more than once"
        );
        *installed = Some(base_description_ops.final_release);
        drop(installed);
        base_description_ops.final_release = Some(slave_final_release);
        base_description_ops
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
            self.description.base_final_release.lock().is_some(),
            "PTY slave participation committed before static hook composition"
        );
        assert!(
            core::ptr::eq(
                tty_file::pty_slave_description(prepared_slave.vfs_file()),
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

/// Exact devpts binding capability installed before the PTY pair becomes
/// live. It exposes only logical retirement and a mount-neutral slave file
/// projection; pair code cannot inspect backend mapping or VFS cache state.
pub(crate) trait PtyBindingOps: Send + Sync {
    fn retire(&self);
    fn peer_path(&self) -> Result<PathRef, SysError>;
}

#[derive(Clone)]
pub(crate) struct PtyBindingCapability {
    inner: Arc<dyn PtyBindingOps>,
}

impl PtyBindingCapability {
    pub(crate) fn new<T: PtyBindingOps + 'static>(binding: Arc<T>) -> Self {
        Self { inner: binding }
    }

    fn retire(&self) {
        self.inner.retire();
    }

    fn peer_path(&self) -> Result<PathRef, SysError> {
        self.inner.peer_path()
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{device::console::open_console_stdin, fs::anony_open_with};

    struct NoopBinding;

    impl PtyBindingOps for NoopBinding {
        fn retire(&self) {}

        fn peer_path(&self) -> Result<PathRef, SysError> {
            Err(SysError::NotFound)
        }
    }

    fn materialize(opened: OpenedFile) -> File {
        let placeholder = open_console_stdin();
        anony_open_with(placeholder.path(), opened).unwrap()
    }

    fn live_pair() -> (LivePtyPair, Arc<PtyMasterDescription>, Arc<File>) {
        let mut prepared = prepare_pair(0).unwrap();
        prepared.compose_description_ops(FileDescOps::default());
        let master = Arc::new(materialize(prepared.take_opened_master()));
        prepared
            .install_cleanup(PtyBindingCapability::new(Arc::new(NoopBinding)))
            .unwrap();
        let PreparedPtyPair {
            pair,
            endpoint,
            enrollment,
            master_description,
            opened_master,
        } = prepared;
        assert!(opened_master.is_none());
        assert!(enrollment.is_none());
        pair.commit_master(&master_description.phase);
        pair.set_slave_locked(false).unwrap();
        let live = LivePtyPair { pair, endpoint };
        (live, master_description, master)
    }

    fn live_slave(pair: &LivePtyPair) -> (Arc<PtySlaveDescription>, Arc<File>) {
        let mut prepared = pair.prepare_slave_description().unwrap();
        prepared.compose_description_ops(FileDescOps::default());
        let slave = Arc::new(materialize(prepared.take_opened_file()));
        let PreparedPtySlaveDescription {
            description,
            opened,
        } = prepared;
        assert!(opened.is_none());
        assert!(description.base_final_release.lock().is_some());
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

    #[kunit]
    fn peer_flag_codec_accepts_only_linux_pty_open_profile() {
        use anemone_abi::fs::linux::open::*;

        let (access, status, fd, no_ctty) =
            parse_peer_open_flags(O_RDWR | O_CLOEXEC | O_NONBLOCK | O_NOCTTY).unwrap();
        assert_eq!(access, OpenAccessMode::ReadWrite);
        assert_eq!(status, FileStatusFlags::NONBLOCK);
        assert!(fd.contains(FdFlags::CLOSE_ON_EXEC));
        assert!(no_ctty);
        assert_eq!(
            parse_peer_open_flags(O_RDONLY).unwrap().0,
            OpenAccessMode::Read
        );
        assert_eq!(
            parse_peer_open_flags(O_WRONLY).unwrap().0,
            OpenAccessMode::Write
        );
        assert_eq!(
            parse_peer_open_flags(O_PATH),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            parse_peer_open_flags(O_ACCMODE),
            Err(SysError::InvalidArgument)
        );
    }

    #[kunit]
    fn pty_ioctl_codec_matches_asm_generic_numbers() {
        assert_eq!(anemone_abi::tty::linux::TIOCGPTN, 0x8004_5430);
        assert_eq!(anemone_abi::tty::linux::TIOCSPTLCK, 0x4004_5431);
        assert_eq!(anemone_abi::tty::linux::TIOCGPTPEER, 0x5441);
    }
}
