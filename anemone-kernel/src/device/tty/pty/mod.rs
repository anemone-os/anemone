use crate::{
    fs::FileMode,
    prelude::*,
    task::files::{FileDesc, FileDescOps},
    utils::any_opaque::AnyOpaque,
};

use super::{
    TtyEndpoint, TtyProgress, TtyWakeHandle, file as tty_file, port::TtyLineSnapshot,
    relation::RelationEnrollment, terminal::Terminal,
};

mod file;
mod pair;

use file::{
    PTY_MASTER_FILE_OPS, PtyMasterFile, master_file, master_final_release, slave_final_release,
};
pub(crate) use pair::PtyPairState;
pub(super) use pair::PtySlaveDescription;
use pair::{DESCRIPTION_PREPARED, PtyMasterDescription};

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
    let master_description = Arc::try_new(PtyMasterDescription::new(
        pair.clone(),
        base_description_ops.final_release,
    ))
    .map_err(|_| SysError::OutOfMemory)?;
    base_description_ops.final_release = Some(master_final_release);
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
        let opened = tty_file::opened_pty_slave_file(
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
