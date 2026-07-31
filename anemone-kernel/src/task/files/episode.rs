use crate::prelude::*;

use super::{
    Fd, FdFlags, FileDesc, FileDescOps, FileStatusFlags, FilesState, LinuxOpenCompat,
    OpenAccessMode, opened_description::ProcFile,
};

/// Opaque POSIX record-lock owner identity for one file-table sharing episode.
///
/// The allocation is the capability itself: it is created with the episode and
/// is not derived from the table storage pointer, participant count, task ID,
/// or opened-description identity. Its only behavioral operation is same-owner
/// comparison; it never carries grants or other record-lock state.
#[derive(Clone, Debug)]
pub(crate) struct PosixLockHolder(Arc<PosixLockHolderIdentity>);

#[derive(Debug)]
struct PosixLockHolderIdentity;

impl PosixLockHolder {
    fn new() -> Self {
        Self(Arc::new(PosixLockHolderIdentity))
    }

    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Debug)]
struct FileTableEpisodeInner {
    /// Semantic participants, not storage observers. Only attach, split, and
    /// explicit detach change this count.
    participants: usize,
    files: FilesState,
}

#[derive(Debug)]
struct FileTableEpisode {
    inner: RwLock<FileTableEpisodeInner>,
    holder: PosixLockHolder,
}

impl FileTableEpisode {
    fn new(files: FilesState) -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(FileTableEpisodeInner {
                participants: 1,
                files,
            }),
            holder: PosixLockHolder::new(),
        })
    }
}

/// The one task-owned capability that makes a task a semantic participant in
/// a file-table sharing episode.
///
/// Cloning the underlying `Arc` is deliberately confined to storage observers;
/// semantic sharing must go through `attach`, and termination must consume this
/// capability through `detach`.
#[derive(Debug)]
pub(crate) struct FileTableParticipation {
    episode: Arc<FileTableEpisode>,
    /// Missing-detach assertion state only. This flag never drives episode
    /// behavior or cleanup; participant truth remains in `episode.inner`.
    attached: bool,
}

impl FileTableParticipation {
    pub(crate) fn new_empty() -> Self {
        Self {
            episode: FileTableEpisode::new(FilesState::new()),
            attached: true,
        }
    }

    fn attach(&self) -> Self {
        let mut inner = self.episode.inner.write();
        assert!(
            inner.participants > 0,
            "cannot attach to a terminal file-table episode"
        );
        inner.participants = inner
            .participants
            .checked_add(1)
            .expect("file-table participant count overflow");
        Self {
            episode: self.episode.clone(),
            attached: true,
        }
    }

    fn fork(&self) -> Self {
        let inner = self.episode.inner.read();
        assert!(
            inner.participants > 0,
            "cannot fork a terminal file-table episode"
        );
        Self {
            episode: FileTableEpisode::new(inner.files.fork()),
            attached: true,
        }
    }

    /// Split this task only when the episode is genuinely shared.
    ///
    /// The episode guard owns both the unique/shared decision and the old
    /// participant withdrawal. A unique replacement is therefore a semantic
    /// no-op and retains the holder identity.
    fn split_if_shared(&mut self) -> bool {
        let fresh_episode = {
            let mut inner = self.episode.inner.write();
            assert!(
                inner.participants > 0,
                "cannot split a terminal file-table episode"
            );
            if inner.participants == 1 {
                return false;
            }

            let fresh = FileTableEpisode::new(inner.files.fork());
            inner.participants -= 1;
            fresh
        };

        self.episode = fresh_episode;
        true
    }

    /// Withdraw one semantic participant and drain the table exactly when it
    /// is the final participant. Opened-description release remains outside
    /// the episode guard because it may enter VFS cleanup and wake waiters.
    fn detach(mut self) -> Vec<Arc<ProcFile>> {
        let closed = {
            let mut inner = self.episode.inner.write();
            assert!(
                inner.participants > 0,
                "file-table participant detached more than once"
            );
            inner.participants -= 1;
            if inner.participants == 0 {
                inner.files.drain_all_published_fds()
            } else {
                Vec::new()
            }
        };
        self.attached = false;
        closed
    }

    fn with_files<R>(&self, f: impl FnOnce(&FilesState) -> R) -> R {
        let inner = self.episode.inner.read();
        assert!(
            inner.participants > 0,
            "terminal file-table episode used by a participant"
        );
        f(&inner.files)
    }

    fn with_files_mut<R>(&self, f: impl FnOnce(&mut FilesState) -> R) -> R {
        let mut inner = self.episode.inner.write();
        assert!(
            inner.participants > 0,
            "terminal file-table episode mutated by a participant"
        );
        f(&mut inner.files)
    }

    fn observer(&self) -> FileTableObserver {
        FileTableObserver {
            episode: self.episode.clone(),
        }
    }

    fn holder(&self) -> PosixLockHolder {
        self.episode.holder.clone()
    }
}

impl Drop for FileTableParticipation {
    fn drop(&mut self) {
        assert!(
            !self.attached,
            "file-table participation dropped without explicit detach"
        );
    }
}

/// Operation/storage capability used by an fd reservation. Cloning it never
/// creates a semantic participant and therefore cannot keep a shared episode
/// alive for behavior. A commit after final detach is rejected before
/// publication; rollback remains idempotent after terminal drain.
#[derive(Clone, Debug)]
struct FileTableObserver {
    episode: Arc<FileTableEpisode>,
}

impl FileTableObserver {
    fn reserve_fd(&self) -> Result<Fd, SysError> {
        let mut inner = self.episode.inner.write();
        assert!(
            inner.participants > 0,
            "cannot reserve an fd in a terminal file-table episode"
        );
        inner.files.reserve_fd()
    }

    fn commit_reserved_fd(&self, fd: Fd, file_desc: Arc<FileDesc>) {
        let mut inner = self.episode.inner.write();
        assert!(
            inner.participants > 0,
            "cannot publish an fd in a terminal file-table episode"
        );
        inner.files.commit_reserved_fd(fd, file_desc);
    }

    fn rollback_reserved_fd(&self, fd: Fd) {
        self.episode.inner.write().files.rollback_reserved_fd(fd);
    }
}

#[derive(Debug)]
pub struct FdReservation {
    table: FileTableObserver,
    fd: Fd,
    active: bool,
}

impl FdReservation {
    pub const fn fd(&self) -> Fd {
        self.fd
    }

    /// Publish a fully prepared file description into the reserved slot.
    ///
    /// Reservation already owns the allocator bit, so commit only transitions
    /// the slot from reserved to visible. It must not allocate or call
    /// file-specific code while holding the fd-table lock.
    pub fn commit(mut self, file_desc: Arc<FileDesc>) -> Fd {
        self.table.commit_reserved_fd(self.fd, file_desc);
        self.active = false;
        self.fd
    }

    pub fn rollback(mut self) {
        self.rollback_inner();
    }

    fn rollback_inner(&mut self) {
        if self.active {
            self.table.rollback_reserved_fd(self.fd);
            self.active = false;
        }
    }
}

impl Drop for FdReservation {
    fn drop(&mut self) {
        self.rollback_inner();
    }
}

impl Task {
    fn with_files<R>(&self, f: impl FnOnce(&FilesState) -> R) -> R {
        let participation = self.files_participation.read();
        participation
            .as_ref()
            .expect("detached task used its file table")
            .with_files(f)
    }

    fn with_files_mut<R>(&self, f: impl FnOnce(&mut FilesState) -> R) -> R {
        let participation = self.files_participation.read();
        participation
            .as_ref()
            .expect("detached task mutated its file table")
            .with_files_mut(f)
    }

    fn release_description_ref(pfile: Arc<ProcFile>) {
        pfile.release_description_ref();
    }

    fn release_description_refs(closed: Vec<Arc<ProcFile>>) {
        for pfile in closed {
            Self::release_description_ref(pfile);
        }
    }

    fn replace_files_participation(&mut self, participation: FileTableParticipation) {
        let old = self
            .files_participation
            .write()
            .replace(participation)
            .expect("new task must own its initial file-table participation");
        Self::release_description_refs(old.detach());
    }

    pub(crate) fn share_files_from(&mut self, parent: &Task) {
        let participation = parent.files_participation.read();
        let shared = participation
            .as_ref()
            .expect("clone parent has detached its file table")
            .attach();
        drop(participation);
        self.replace_files_participation(shared);
    }

    pub(crate) fn fork_files_from(&mut self, parent: &Task) {
        let participation = parent.files_participation.read();
        let forked = participation
            .as_ref()
            .expect("fork parent has detached its file table")
            .fork();
        drop(participation);
        self.replace_files_participation(forked);
    }

    pub(crate) fn split_files_if_shared(&self) -> bool {
        self.files_participation
            .write()
            .as_mut()
            .expect("detached task cannot split its file table")
            .split_if_shared()
    }

    pub(crate) fn detach_files_for_exit(&self) {
        assert!(
            IntrArch::local_intr_enabled(),
            "fd-table exit cleanup must run with interrupts enabled"
        );
        assert!(
            allow_preempt(),
            "fd-table exit cleanup must run in a sleepable context"
        );

        let participation = self
            .files_participation
            .write()
            .take()
            .expect("task file-table participation detached more than once");
        Self::release_description_refs(participation.detach());
    }

    pub fn open_fd(
        &self,
        file: File,
        access: OpenAccessMode,
        status_flags: FileStatusFlags,
        compat: LinuxOpenCompat,
        fd_flags: FdFlags,
    ) -> Result<Fd, SysError> {
        self.with_files_mut(|files| files.open_fd(file, access, status_flags, compat, fd_flags))
    }

    pub fn open_fd_with_description_ops(
        &self,
        file: File,
        access: OpenAccessMode,
        status_flags: FileStatusFlags,
        compat: LinuxOpenCompat,
        fd_flags: FdFlags,
        description_ops: FileDescOps,
    ) -> Result<Fd, SysError> {
        self.with_files_mut(|files| {
            files.open_fd_with_description_ops(
                file,
                access,
                status_flags,
                compat,
                fd_flags,
                description_ops,
            )
        })
    }

    pub fn reserve_fd(&self) -> Result<FdReservation, SysError> {
        let participation = self.files_participation.read();
        let table = participation
            .as_ref()
            .expect("detached task cannot reserve an fd")
            .observer();
        let fd = table.reserve_fd()?;
        Ok(FdReservation {
            table,
            fd,
            active: true,
        })
    }

    pub fn get_fd(&self, fd: Fd) -> Result<Arc<FileDesc>, SysError> {
        self.with_files(|files| files.get_fd(fd))
    }

    pub fn opened_fd_numbers_snapshot(&self) -> Vec<Fd> {
        self.with_files(FilesState::opened_fd_numbers_snapshot)
    }

    pub fn close_fd(&self, fd: Fd) -> Result<(), SysError> {
        let pfile = self.with_files_mut(|files| files.close_fd(fd))?;
        Self::release_description_ref(pfile);
        Ok(())
    }

    pub fn dup(&self, old_fd: Fd) -> Result<Fd, SysError> {
        self.with_files_mut(|files| files.dup(old_fd))
    }

    pub fn dup_ge_than(
        &self,
        old_fd: Fd,
        min_new_fd: Fd,
        close_on_exec: bool,
    ) -> Result<Fd, SysError> {
        self.with_files_mut(|files| files.dup_ge_than(old_fd, min_new_fd, close_on_exec))
    }

    pub fn dup3(&self, old_fd: Fd, new_fd: Fd, flags: FdFlags) -> Result<Fd, SysError> {
        let closed = self.with_files_mut(|files| files.dup3(old_fd, new_fd, flags))?;
        Self::release_description_refs(closed);
        Ok(new_fd)
    }

    pub fn close_cloexec_fds(&self) {
        let closed = self.with_files_mut(FilesState::close_on_exec);
        Self::release_description_refs(closed);
    }

    pub fn close_range(
        &self,
        first: u32,
        last: u32,
        flags: crate::fs::api::close::CloseRangeFlags,
    ) {
        if flags.contains(crate::fs::api::close::CloseRangeFlags::UNSHARE) {
            self.split_files_if_shared();
        }

        if flags.contains(crate::fs::api::close::CloseRangeFlags::CLOEXEC) {
            self.with_files(|files| files.set_close_on_exec_range(first, last));
        } else {
            let closed = self.with_files_mut(|files| files.close_range(first, last));
            Self::release_description_refs(closed);
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn open_root(participation: &FileTableParticipation) -> Fd {
        participation
            .with_files_mut(|files| {
                files.open_fd(
                    vfs_open(Path::new("/")).unwrap(),
                    OpenAccessMode::Read,
                    FileStatusFlags::empty(),
                    LinuxOpenCompat::empty(),
                    FdFlags::empty(),
                )
            })
            .unwrap()
    }

    fn release_all(closed: Vec<Arc<ProcFile>>) {
        for pfile in closed {
            pfile.release_description_ref();
        }
    }

    #[kunit]
    fn posix_holder_fork_and_share_follow_episode_identity() {
        let parent = FileTableParticipation::new_empty();
        let forked = parent.fork();
        let shared = parent.attach();

        assert!(!parent.holder().same_identity(&forked.holder()));
        assert!(parent.holder().same_identity(&shared.holder()));

        release_all(forked.detach());
        release_all(shared.detach());
        release_all(parent.detach());
    }

    #[kunit]
    fn posix_holder_split_changes_only_a_shared_episode() {
        let mut caller = FileTableParticipation::new_empty();
        let unique_holder = caller.holder();
        assert!(!caller.split_if_shared());
        assert!(unique_holder.same_identity(&caller.holder()));

        let remaining = caller.attach();
        let shared_holder = remaining.holder();
        assert!(caller.split_if_shared());
        assert!(!shared_holder.same_identity(&caller.holder()));
        assert!(shared_holder.same_identity(&remaining.holder()));

        release_all(caller.detach());
        release_all(remaining.detach());
    }

    #[kunit]
    fn final_detach_ignores_storage_observers_and_drains_once() {
        let first = FileTableParticipation::new_empty();
        let fd = open_root(&first);
        let capability = first.with_files(|files| {
            files
                .get_fd(fd)
                .unwrap()
                .opened_description_capability()
                .unwrap()
        });
        let second = first.attach();
        let observer = first.observer();
        let another_observer = observer.clone();

        assert!(first.detach().is_empty());
        assert!(capability.try_lease().is_some());

        let closed = second.detach();
        assert_eq!(closed.len(), 1);
        release_all(closed);
        assert!(capability.try_lease().is_none());
        assert!(observer.episode.inner.read().files.get_fd(fd).is_err());
        assert!(
            another_observer
                .episode
                .inner
                .read()
                .files
                .opened_fd_numbers_snapshot()
                .is_empty()
        );
    }
}
