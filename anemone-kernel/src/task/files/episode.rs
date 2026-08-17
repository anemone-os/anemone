use crate::prelude::*;

use super::{
    Fd, FdAllocCeiling, FdFlags, FileDesc, FileDescOps, FileStatusFlags, FileTable,
    LinuxOpenCompat, OpenAccessMode, OpenedDescriptionBundle,
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

    #[cfg(feature = "kunit")]
    pub(crate) fn new_for_kunit() -> Self {
        Self::new()
    }

    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// One operation's exact POSIX-lock owner and fd-slot capability.
///
/// The holder and slot are captured under the same episode guard. The binding
/// is deliberately not cloneable: it carries no fd number, range, table
/// access, or participation truth, and `FileDesc::published` remains the sole
/// commit-time liveness source.
#[derive(Debug)]
pub(crate) struct PosixLockBinding {
    holder: PosixLockHolder,
    file_desc: Arc<FileDesc>,
}

impl PosixLockBinding {
    fn new(holder: PosixLockHolder, file_desc: Arc<FileDesc>) -> Self {
        Self { holder, file_desc }
    }

    pub(crate) fn holder(&self) -> &PosixLockHolder {
        &self.holder
    }

    pub(crate) fn file(&self) -> &Arc<File> {
        self.file_desc.vfs_file()
    }

    pub(crate) fn access_mode(&self) -> OpenAccessMode {
        self.file_desc.access_mode()
    }

    pub(crate) fn is_path_only(&self) -> bool {
        self.file_desc.is_path_only()
    }

    pub(crate) fn is_live(&self) -> bool {
        self.file_desc.is_published()
    }

    pub(crate) fn position(&self) -> usize {
        self.file().pos()
    }

    pub(crate) fn inode_size(&self) -> u64 {
        self.file().inode().size()
    }

    pub(crate) fn is_regular(&self) -> bool {
        self.file().inode().ty() == InodeType::Regular
    }

    fn release_description_ref(&self) {
        self.file_desc.release_description_ref();
    }
}

#[derive(Debug)]
struct FileTableEpisodeInner {
    /// Semantic participants, not storage observers. Only attach, split, and
    /// explicit detach change this count.
    participants: usize,
    table: FileTable,
}

#[derive(Debug)]
struct FileTableEpisode {
    inner: RwLock<FileTableEpisodeInner>,
    holder: PosixLockHolder,
}

impl FileTableEpisode {
    fn new(table: FileTable) -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(FileTableEpisodeInner {
                participants: 1,
                table,
            }),
            holder: PosixLockHolder::new(),
        })
    }
}

/// Task-owned file state for one file-table sharing episode.
///
/// This facade is also the task's semantic participation capability. Cloning
/// the underlying `Arc` is deliberately confined to storage observers;
/// semantic sharing must go through `attach`, and termination must consume
/// this state through `detach`.
#[derive(Debug)]
pub(crate) struct FilesState {
    episode: Arc<FileTableEpisode>,
    /// Missing-detach assertion state only. This flag never drives episode
    /// behavior or cleanup; participant truth remains in `episode.inner`.
    attached: bool,
}

impl FilesState {
    pub(crate) fn new_empty() -> Self {
        Self {
            episode: FileTableEpisode::new(FileTable::new()),
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
            episode: FileTableEpisode::new(inner.table.fork()),
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

            let fresh = FileTableEpisode::new(inner.table.fork());
            inner.participants -= 1;
            fresh
        };

        self.episode = fresh_episode;
        true
    }

    /// Withdraw one semantic participant and drain the table exactly when it
    /// is the final participant. Opened-description release remains outside
    /// the episode guard because it may enter VFS cleanup and wake waiters.
    fn detach(mut self) -> Vec<PosixLockBinding> {
        let closed = {
            let mut inner = self.episode.inner.write();
            assert!(
                inner.participants > 0,
                "file-table participant detached more than once"
            );
            inner.participants -= 1;
            if inner.participants == 0 {
                inner
                    .table
                    .drain_all_published_fds()
                    .into_iter()
                    .map(|file_desc| PosixLockBinding::new(self.episode.holder.clone(), file_desc))
                    .collect()
            } else {
                Vec::new()
            }
        };
        self.attached = false;
        closed
    }

    fn with_table<R>(&self, f: impl FnOnce(&FileTable) -> R) -> R {
        let inner = self.episode.inner.read();
        assert!(
            inner.participants > 0,
            "terminal file-table episode used by a participant"
        );
        f(&inner.table)
    }

    fn with_table_mut<R>(&self, f: impl FnOnce(&mut FileTable) -> R) -> R {
        let mut inner = self.episode.inner.write();
        assert!(
            inner.participants > 0,
            "terminal file-table episode mutated by a participant"
        );
        f(&mut inner.table)
    }

    fn observer(&self) -> FileTableObserver {
        FileTableObserver {
            episode: self.episode.clone(),
        }
    }

    fn holder(&self) -> PosixLockHolder {
        self.episode.holder.clone()
    }

    fn binding(&self, fd: Fd) -> Result<PosixLockBinding, SysError> {
        let inner = self.episode.inner.read();
        assert!(
            inner.participants > 0,
            "terminal file-table episode used for POSIX lock binding"
        );
        let file_desc = inner.table.get_fd(fd)?;
        Ok(PosixLockBinding::new(
            self.episode.holder.clone(),
            file_desc,
        ))
    }

    fn with_removed<R>(
        &self,
        remove: impl FnOnce(&mut FileTable) -> Result<R, SysError>,
        bind: impl FnOnce(R, &PosixLockHolder) -> Vec<PosixLockBinding>,
    ) -> Result<Vec<PosixLockBinding>, SysError> {
        let mut inner = self.episode.inner.write();
        assert!(
            inner.participants > 0,
            "terminal file-table episode used for fd removal"
        );
        let removed = remove(&mut inner.table)?;
        Ok(bind(removed, &self.episode.holder))
    }
}

impl Drop for FilesState {
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
    fn reserve_fd(&self, ceiling: FdAllocCeiling) -> Result<Fd, SysError> {
        let mut inner = self.episode.inner.write();
        assert!(
            inner.participants > 0,
            "cannot reserve an fd in a terminal file-table episode"
        );
        inner.table.reserve_fd(ceiling)
    }

    fn commit_reserved_fd(&self, fd: Fd, file_desc: Arc<FileDesc>) {
        let mut inner = self.episode.inner.write();
        assert!(
            inner.participants > 0,
            "cannot publish an fd in a terminal file-table episode"
        );
        inner.table.commit_reserved_fd(fd, file_desc);
    }

    fn rollback_reserved_fd(&self, fd: Fd) {
        self.episode.inner.write().table.rollback_reserved_fd(fd);
    }

    fn reserve_fds_up_to(
        &self,
        ceiling: FdAllocCeiling,
        count: usize,
    ) -> Result<Vec<Fd>, SysError> {
        let mut fds = Vec::new();
        fds.try_reserve_exact(count)
            .map_err(|_| SysError::OutOfMemory)?;
        let mut inner = self.episode.inner.write();
        assert!(
            inner.participants > 0,
            "cannot reserve fds in a terminal file-table episode"
        );
        for _ in 0..count {
            match inner.table.reserve_fd(ceiling) {
                Ok(fd) => fds.push(fd),
                Err(SysError::NoMoreFd) => break,
                Err(error) => {
                    inner.table.rollback_reserved_fds(&fds);
                    return Err(error);
                },
            }
        }
        Ok(fds)
    }

    fn commit_reserved_fds(&self, entries: Vec<(Fd, Arc<FileDesc>)>) {
        let mut inner = self.episode.inner.write();
        assert!(
            inner.participants > 0,
            "cannot publish fds in a terminal file-table episode"
        );
        inner.table.commit_reserved_fds(entries);
    }

    fn rollback_reserved_fds(&self, fds: &[Fd]) {
        self.episode.inner.write().table.rollback_reserved_fds(fds);
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

/// Receiver-local all-or-none reservation for one SCM_RIGHTS projection.
///
/// The plan owns both the unpublished fd slots and the detached transfer
/// bundle. Failed output copy drops the plan, rolling slots back before any
/// transfer can run terminal cleanup. Successful commit prepares every
/// descriptor outside the table guard and publishes the complete prefix in one
/// table episode.
#[derive(Debug)]
pub(crate) struct OpenedDescriptionInstallPlan {
    table: FileTableObserver,
    fds: Vec<Fd>,
    bundle: Option<OpenedDescriptionBundle>,
    fd_flags: FdFlags,
    active: bool,
}

impl OpenedDescriptionInstallPlan {
    pub(crate) fn fds(&self) -> &[Fd] {
        &self.fds
    }

    pub(crate) fn source_count(&self) -> usize {
        self.bundle
            .as_ref()
            .expect("committed transfer install plan inspected")
            .len()
    }

    pub(crate) fn commit(mut self) {
        let bundle = self
            .bundle
            .take()
            .expect("opened-description install plan committed twice");
        let mut transfers = bundle.into_transfers();
        let mut entries = Vec::with_capacity(self.fds.len());
        for &fd in &self.fds {
            let transfer = transfers
                .next()
                .expect("fd reservation exceeded transfer bundle");
            entries.push((fd, FileDesc::from_transfer(transfer, self.fd_flags)));
        }
        // Uninstalled suffixes are intentionally discarded after truncation.
        // Keep that cleanup outside the fd-table guard.
        self.table.commit_reserved_fds(entries);
        self.active = false;
        drop(transfers);
    }

    fn rollback_inner(&mut self) {
        if self.active {
            self.table.rollback_reserved_fds(&self.fds);
            self.active = false;
        }
    }
}

impl Drop for OpenedDescriptionInstallPlan {
    fn drop(&mut self) {
        self.rollback_inner();
    }
}

impl Task {
    pub(crate) fn fd_alloc_ceiling(&self) -> FdAllocCeiling {
        self.get_thread_group().nofile_alloc_ceiling()
    }

    fn with_table<R>(&self, f: impl FnOnce(&FileTable) -> R) -> R {
        let files_state = self.files_state.read();
        files_state
            .as_ref()
            .expect("detached task used its file table")
            .with_table(f)
    }

    fn with_table_mut<R>(&self, f: impl FnOnce(&mut FileTable) -> R) -> R {
        let files_state = self.files_state.read();
        files_state
            .as_ref()
            .expect("detached task mutated its file table")
            .with_table_mut(f)
    }

    fn finish_removed_binding(binding: PosixLockBinding) {
        // The fd-table publication is already withdrawn and all table/episode
        // guards are gone before VFS cleanup. Opened-description retirement is
        // last because it may enter backend final-release code.
        crate::fs::retire_posix_locks(&binding);
        binding.release_description_ref();
    }

    fn finish_removed_bindings(closed: Vec<PosixLockBinding>) {
        for binding in closed {
            Self::finish_removed_binding(binding);
        }
    }

    fn replace_files_state(&mut self, files_state: FilesState) {
        let old = self
            .files_state
            .write()
            .replace(files_state)
            .expect("new task must own its initial files state");
        Self::finish_removed_bindings(old.detach());
    }

    pub(crate) fn share_files_from(&mut self, parent: &Task) {
        let files_state = parent.files_state.read();
        let shared = files_state
            .as_ref()
            .expect("clone parent has detached its file table")
            .attach();
        drop(files_state);
        self.replace_files_state(shared);
    }

    pub(crate) fn fork_files_from(&mut self, parent: &Task) {
        let files_state = parent.files_state.read();
        let forked = files_state
            .as_ref()
            .expect("fork parent has detached its file table")
            .fork();
        drop(files_state);
        self.replace_files_state(forked);
    }

    pub(crate) fn split_files_if_shared(&self) -> bool {
        self.files_state
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

        let files_state = self
            .files_state
            .write()
            .take()
            .expect("task file-table participation detached more than once");
        Self::finish_removed_bindings(files_state.detach());
    }

    pub fn open_fd(
        &self,
        file: File,
        access: OpenAccessMode,
        status_flags: FileStatusFlags,
        compat: LinuxOpenCompat,
        fd_flags: FdFlags,
    ) -> Result<Fd, SysError> {
        let ceiling = self.fd_alloc_ceiling();
        self.with_table_mut(|table| {
            table.open_fd(ceiling, file, access, status_flags, compat, fd_flags)
        })
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
        let ceiling = self.fd_alloc_ceiling();
        self.with_table_mut(|table| {
            table.open_fd_with_description_ops(
                ceiling,
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
        let ceiling = self.fd_alloc_ceiling();
        let files_state = self.files_state.read();
        let table = files_state
            .as_ref()
            .expect("detached task cannot reserve an fd")
            .observer();
        let fd = table.reserve_fd(ceiling)?;
        Ok(FdReservation {
            table,
            fd,
            active: true,
        })
    }

    /// Capture exact fd slots under one table episode and return only opaque
    /// semantic transfer references. Partial capture cleanup happens after the
    /// table guard is released.
    pub(crate) fn capture_opened_descriptions(
        &self,
        fds: &[Fd],
    ) -> Result<OpenedDescriptionBundle, SysError> {
        let mut bundle = OpenedDescriptionBundle::try_with_capacity(fds.len())?;
        let result = self.with_table(|table| {
            for &fd in fds {
                let file_desc = table.get_fd(fd)?;
                bundle.push(file_desc.capture_transfer());
            }
            Ok(())
        });
        result?;
        Ok(bundle)
    }

    pub(crate) fn prepare_opened_description_install(
        &self,
        bundle: OpenedDescriptionBundle,
        maximum: usize,
        fd_flags: FdFlags,
    ) -> Result<OpenedDescriptionInstallPlan, SysError> {
        let ceiling = self.fd_alloc_ceiling();
        let files_state = self.files_state.read();
        let table = files_state
            .as_ref()
            .expect("detached task cannot reserve transferred fds")
            .observer();
        let fds = table.reserve_fds_up_to(ceiling, maximum.min(bundle.len()))?;
        Ok(OpenedDescriptionInstallPlan {
            table,
            fds,
            bundle: Some(bundle),
            fd_flags,
            active: true,
        })
    }

    pub fn get_fd(&self, fd: Fd) -> Result<Arc<FileDesc>, SysError> {
        self.with_table(|table| table.get_fd(fd))
    }

    pub(crate) fn posix_lock_binding(&self, fd: Fd) -> Result<PosixLockBinding, SysError> {
        let files_state = self.files_state.read();
        files_state
            .as_ref()
            .expect("detached task cannot capture a POSIX lock binding")
            .binding(fd)
    }

    pub fn opened_fd_numbers_snapshot(&self) -> Vec<Fd> {
        self.with_table(FileTable::opened_fd_numbers_snapshot)
    }

    pub fn close_fd(&self, fd: Fd) -> Result<(), SysError> {
        let closed = {
            let files_state = self.files_state.read();
            files_state
                .as_ref()
                .expect("detached task cannot close an fd")
                .with_removed(
                    |table| table.close_fd(fd),
                    |file_desc, holder| vec![PosixLockBinding::new(holder.clone(), file_desc)],
                )?
        };
        Self::finish_removed_bindings(closed);
        Ok(())
    }

    pub fn dup(&self, old_fd: Fd) -> Result<Fd, SysError> {
        let ceiling = self.fd_alloc_ceiling();
        self.with_table_mut(|table| table.dup(old_fd, ceiling))
    }

    pub fn dup_ge_than(
        &self,
        old_fd: Fd,
        min_new_fd: Fd,
        close_on_exec: bool,
    ) -> Result<Fd, SysError> {
        let ceiling = self.fd_alloc_ceiling();
        self.with_table_mut(|table| table.dup_ge_than(old_fd, min_new_fd, close_on_exec, ceiling))
    }

    pub fn dup3(&self, old_fd: Fd, new_fd: Fd, flags: FdFlags) -> Result<Fd, SysError> {
        let ceiling = self.fd_alloc_ceiling();
        let closed = {
            let files_state = self.files_state.read();
            files_state
                .as_ref()
                .expect("detached task cannot duplicate an fd")
                .with_removed(
                    |table| table.dup3(old_fd, new_fd, flags, ceiling),
                    |file_descs, holder| {
                        file_descs
                            .into_iter()
                            .map(|file_desc| PosixLockBinding::new(holder.clone(), file_desc))
                            .collect()
                    },
                )?
        };
        Self::finish_removed_bindings(closed);
        Ok(new_fd)
    }

    pub fn close_cloexec_fds(&self) {
        let closed = {
            let files_state = self.files_state.read();
            files_state
                .as_ref()
                .expect("detached task cannot close CLOEXEC fds")
                .with_removed(
                    |table| Ok(table.close_on_exec()),
                    |file_descs, holder| {
                        file_descs
                            .into_iter()
                            .map(|file_desc| PosixLockBinding::new(holder.clone(), file_desc))
                            .collect()
                    },
                )
                .expect("CLOEXEC removal cannot fail")
        };
        Self::finish_removed_bindings(closed);
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
            self.with_table(|table| table.set_close_on_exec_range(first, last));
        } else {
            let closed = {
                let files_state = self.files_state.read();
                files_state
                    .as_ref()
                    .expect("detached task cannot close an fd range")
                    .with_removed(
                        |table| Ok(table.close_range(first, last)),
                        |file_descs, holder| {
                            file_descs
                                .into_iter()
                                .map(|file_desc| PosixLockBinding::new(holder.clone(), file_desc))
                                .collect()
                        },
                    )
                    .expect("close_range removal cannot fail")
            };
            Self::finish_removed_bindings(closed);
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::fs::{
        InodePerm, PosixLockMode, PosixLockRange, PosixLockSetOutcome, set_posix_lock,
        vfs_touch_as_root, vfs_unlink,
    };

    fn open_root(files_state: &FilesState) -> Fd {
        files_state
            .with_table_mut(|table| {
                table.open_fd(
                    FdAllocCeiling::new(MAX_FD_PER_PROCESS).unwrap(),
                    vfs_open(Path::new("/")).unwrap(),
                    OpenAccessMode::Read,
                    FileStatusFlags::empty(),
                    LinuxOpenCompat::empty(),
                    FdFlags::empty(),
                )
            })
            .unwrap()
    }

    fn release_all(closed: Vec<PosixLockBinding>) {
        for binding in closed {
            crate::fs::retire_posix_locks(&binding);
            binding.release_description_ref();
        }
    }

    #[kunit]
    fn posix_binding_rejects_late_commit_and_fd_reuse_gets_a_fresh_slot() {
        let path = Path::new("/kunit-posix-binding-liveness");
        let _created = vfs_touch_as_root(path, InodePerm::all_rwx()).unwrap();
        let files = FilesState::new_empty();
        let fd = files
            .with_table_mut(|table| {
                table.open_fd(
                    FdAllocCeiling::new(MAX_FD_PER_PROCESS).unwrap(),
                    vfs_open(path).unwrap(),
                    OpenAccessMode::ReadWrite,
                    FileStatusFlags::empty(),
                    LinuxOpenCompat::empty(),
                    FdFlags::empty(),
                )
            })
            .unwrap();
        let old = files.binding(fd).unwrap();
        let range = PosixLockRange::finite(0, 1);
        assert_eq!(
            set_posix_lock(&old, range, PosixLockMode::Write, 1, false),
            PosixLockSetOutcome::Applied
        );

        let closed = files
            .with_removed(
                |table| table.close_fd(fd),
                |file_desc, holder| vec![PosixLockBinding::new(holder.clone(), file_desc)],
            )
            .unwrap();
        assert_eq!(
            set_posix_lock(&old, range, PosixLockMode::Write, 1, false),
            PosixLockSetOutcome::BindingRetired
        );
        release_all(closed);

        let reused = files
            .with_table_mut(|table| {
                table.open_fd(
                    FdAllocCeiling::new(MAX_FD_PER_PROCESS).unwrap(),
                    vfs_open(path).unwrap(),
                    OpenAccessMode::ReadWrite,
                    FileStatusFlags::empty(),
                    LinuxOpenCompat::empty(),
                    FdFlags::empty(),
                )
            })
            .unwrap();
        assert_eq!(reused, fd);
        let fresh = files.binding(reused).unwrap();
        assert_eq!(
            set_posix_lock(&fresh, range, PosixLockMode::Write, 2, false),
            PosixLockSetOutcome::Applied
        );

        release_all(files.detach());
        vfs_unlink(path).unwrap();
    }

    #[kunit]
    fn posix_holder_fork_and_share_follow_episode_identity() {
        let parent = FilesState::new_empty();
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
        let mut caller = FilesState::new_empty();
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
        let first = FilesState::new_empty();
        let fd = open_root(&first);
        let capability = first.with_table(|table| {
            table
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
        assert!(observer.episode.inner.read().table.get_fd(fd).is_err());
        assert!(
            another_observer
                .episode
                .inner
                .read()
                .table
                .opened_fd_numbers_snapshot()
                .is_empty()
        );
    }

    #[kunit]
    fn transfer_install_plan_honors_ceiling_rolls_back_and_commits_cloexec() {
        let files = FilesState::new_empty();
        let observer = files.observer();
        let ceiling = FdAllocCeiling::new(2).unwrap();

        let (aborted_bundle, aborted_identity) = OpenedDescriptionBundle::for_kunit(3);
        let aborted_fds = observer
            .reserve_fds_up_to(ceiling, aborted_bundle.len())
            .unwrap();
        assert_eq!(aborted_fds, [Fd::new(0).unwrap(), Fd::new(1).unwrap()]);
        let aborted = OpenedDescriptionInstallPlan {
            table: observer.clone(),
            fds: aborted_fds,
            bundle: Some(aborted_bundle),
            fd_flags: FdFlags::CLOSE_ON_EXEC,
            active: true,
        };
        assert!(
            files
                .with_table(FileTable::opened_fd_numbers_snapshot)
                .is_empty()
        );
        drop(aborted);
        assert!(
            files
                .with_table(FileTable::opened_fd_numbers_snapshot)
                .is_empty()
        );
        assert!(aborted_identity.try_lease().is_none());

        let (committed_bundle, committed_identity) = OpenedDescriptionBundle::for_kunit(3);
        let committed_fds = observer
            .reserve_fds_up_to(ceiling, committed_bundle.len())
            .unwrap();
        let committed = OpenedDescriptionInstallPlan {
            table: observer,
            fds: committed_fds.clone(),
            bundle: Some(committed_bundle),
            fd_flags: FdFlags::CLOSE_ON_EXEC,
            active: true,
        };
        committed.commit();
        assert_eq!(
            files.with_table(FileTable::opened_fd_numbers_snapshot),
            committed_fds
        );
        files.with_table(|table| {
            for fd in &committed_fds {
                assert_eq!(
                    table.get_fd(*fd).unwrap().fd_flags(),
                    FdFlags::CLOSE_ON_EXEC
                );
            }
        });
        // The uninstalled suffix was discarded at commit; the two published
        // aliases keep the shared description live until final table detach.
        assert!(committed_identity.try_lease().is_some());
        release_all(files.detach());
        assert!(committed_identity.try_lease().is_none());
    }
}
