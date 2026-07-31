use crate::{
    prelude::{handler::TryFromSyscallArg, *},
    utils::bitmap::Bitmap,
};

use super::{
    descriptor::{FdFlags, FileDesc, FileStatusFlags, LinuxOpenCompat, OpenAccessMode},
    opened_description::{FileDescOps, ProcFile},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Fd(u32);

impl Fd {
    /// Create a new Fd from a raw u32 value.
    ///
    /// Returns None if the value is too large to be a valid fd number.
    pub const fn new(fd: u32) -> Option<Self> {
        if fd >= i32::MAX as u32 {
            None
        } else {
            Some(Self(fd))
        }
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

impl TryFromSyscallArg for Fd {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = i32::try_from_syscall_arg(raw)? as u32;
        Fd::new(raw).ok_or(SysError::BadFileDescriptor)
    }
}

#[derive(Debug)]
pub struct FdReservation {
    files_state: Arc<RwLock<FilesState>>,
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
        {
            let mut files_state = self.files_state.write();
            files_state.commit_reserved_fd(self.fd, file_desc);
        }
        self.active = false;
        self.fd
    }

    pub fn rollback(mut self) {
        self.rollback_inner();
    }

    fn rollback_inner(&mut self) {
        if self.active {
            self.files_state.write().rollback_reserved_fd(self.fd);
            self.active = false;
        }
    }
}

impl Drop for FdReservation {
    fn drop(&mut self) {
        self.rollback_inner();
    }
}

#[derive(Debug)]
pub struct FilesState {
    // `bitmap` is the allocator truth source: a set bit means the slot is
    // either published or reserved. `reserved_bitmap` marks the unpublished
    // subset. Published slots are the only ones visible through `fds`.
    bitmap: Bitmap<{ MAX_FD_PER_PROCESS / 64 }>,
    reserved_bitmap: Bitmap<{ MAX_FD_PER_PROCESS / 64 }>,
    fds: Vec<Option<Arc<FileDesc>>>,
}
// fd alloc
impl FilesState {
    fn alloc(&mut self) -> Result<Fd, SysError> {
        if let Some(fd_idx) = self.bitmap.find_and_set_first_zero() {
            let fd = Fd::new(fd_idx as u32).unwrap();
            debug_assert!(self.fds[fd_idx].is_none());
            Ok(fd)
        } else {
            Err(SysError::NoMoreFd)
        }
    }

    fn alloc_ge_than(&mut self, min_fd: Fd) -> Result<Fd, SysError> {
        if min_fd.raw() as usize >= self.fds.len() {
            return Err(SysError::BadFileDescriptor);
        }

        if let Some(fd_idx) = self
            .bitmap
            .find_and_set_first_zero_from(min_fd.raw() as usize)
        {
            let fd = Fd::new(fd_idx as u32).unwrap();
            debug_assert!(self.fds[fd_idx].is_none());
            Ok(fd)
        } else {
            Err(SysError::NoMoreFd)
        }
    }

    fn alloc_at(&mut self, fd: Fd) -> Result<(), SysError> {
        if fd.raw() as usize >= self.fds.len() {
            return Err(SysError::BadFileDescriptor);
        }

        if self.bitmap.test(fd.raw() as usize) {
            Err(SysError::NoMoreFd)
        } else {
            self.bitmap.set(fd.raw() as usize);
            debug_assert!(self.fds[fd.raw() as usize].is_none());
            Ok(())
        }
    }

    fn publish_fd_desc(&mut self, fd: Fd, file_desc: Arc<FileDesc>) {
        let idx = fd.raw() as usize;
        assert!(self.bitmap.test(idx), "publishing fd without allocator bit");
        assert!(
            !self.reserved_bitmap.test(idx),
            "regular publish cannot target a reserved fd slot"
        );
        assert!(self.fds[idx].is_none(), "publishing over live fd slot");
        file_desc.publish_to_fd_table();
        self.fds[idx] = Some(file_desc);
    }

    fn recycle(&mut self, fd: Fd) -> Arc<ProcFile> {
        debug_assert!(fd < Fd(MAX_FD_PER_PROCESS as u32));
        debug_assert!(self.fds[fd.raw() as usize].is_some());
        let file_desc = self.fds[fd.raw() as usize].take().unwrap();
        self.bitmap.clear(fd.raw() as usize);
        file_desc.unpublish_from_fd_table()
    }

    fn reserve_fd(&mut self) -> Result<Fd, SysError> {
        let fd = self.alloc()?;
        let idx = fd.raw() as usize;
        assert!(self.fds[idx].is_none());
        assert!(!self.reserved_bitmap.test(idx));
        self.reserved_bitmap.set(idx);
        Ok(fd)
    }

    fn commit_reserved_fd(&mut self, fd: Fd, file_desc: Arc<FileDesc>) {
        let idx = fd.raw() as usize;
        assert!(idx < self.fds.len(), "reserved fd index out of bounds");
        assert!(
            self.bitmap.test(idx) && self.reserved_bitmap.test(idx),
            "committing a non-reserved fd slot"
        );
        assert!(self.fds[idx].is_none(), "reserved fd slot became visible");
        file_desc.publish_to_fd_table();
        self.fds[idx] = Some(file_desc);
        self.reserved_bitmap.clear(idx);
    }

    fn rollback_reserved_fd(&mut self, fd: Fd) {
        let idx = fd.raw() as usize;
        assert!(idx < self.fds.len(), "reserved fd index out of bounds");
        assert!(
            self.fds[idx].is_none(),
            "rollback cannot target a published fd slot"
        );
        if self.reserved_bitmap.test(idx) {
            assert!(self.bitmap.test(idx), "reserved slot missing allocator bit");
            self.reserved_bitmap.clear(idx);
            self.bitmap.clear(idx);
        }
    }
}

// operations
impl FilesState {
    pub fn new() -> Self {
        Self {
            bitmap: Bitmap::new(),
            reserved_bitmap: Bitmap::new(),
            fds: vec![None; MAX_FD_PER_PROCESS],
        }
    }

    pub(super) fn open_fd(
        &mut self,
        file: File,
        access: OpenAccessMode,
        status_flags: FileStatusFlags,
        compat: LinuxOpenCompat,
        fd_flags: FdFlags,
    ) -> Result<Fd, SysError> {
        self.open_fd_with_description_ops(
            file,
            access,
            status_flags,
            compat,
            fd_flags,
            FileDescOps::default(),
        )
    }

    fn open_fd_with_description_ops(
        &mut self,
        file: File,
        access: OpenAccessMode,
        status_flags: FileStatusFlags,
        compat: LinuxOpenCompat,
        fd_flags: FdFlags,
        description_ops: FileDescOps,
    ) -> Result<Fd, SysError> {
        let fd = self.alloc()?;
        let file_desc = FileDesc::new_opened(
            file,
            access,
            status_flags,
            compat,
            fd_flags,
            description_ops,
        );
        self.publish_fd_desc(fd, file_desc);
        Ok(fd)
    }

    pub(super) fn close_fd(&mut self, fd: Fd) -> Result<Arc<ProcFile>, SysError> {
        if fd.raw() as usize >= self.fds.len() {
            return Err(SysError::BadFileDescriptor);
        }

        if self.fds[fd.raw() as usize].is_some() {
            Ok(self.recycle(fd))
        } else {
            Err(SysError::BadFileDescriptor)
        }
    }

    fn close_range(&mut self, first: u32, last: u32) -> Vec<Arc<ProcFile>> {
        let first = first as usize;
        if first >= self.fds.len() {
            return Vec::new();
        }

        let last = core::cmp::min(last as usize, self.fds.len() - 1);
        if first > last {
            return Vec::new();
        }

        let mut fds = Vec::new();
        for fd in first..=last {
            if self.fds[fd].is_some() {
                fds.push(Fd::new(fd as u32).unwrap());
            }
        }

        let mut closed = Vec::new();
        for fd in fds {
            if let Ok(pfile) = self.close_fd(fd) {
                closed.push(pfile);
            }
        }
        closed
    }

    fn set_close_on_exec_range(&self, first: u32, last: u32) {
        let first = first as usize;
        if first >= self.fds.len() {
            return;
        }

        let last = core::cmp::min(last as usize, self.fds.len() - 1);
        if first > last {
            return;
        }

        let mut fds = Vec::new();
        for fd in first..=last {
            if let Some(file_desc) = &self.fds[fd] {
                fds.push(file_desc.clone());
            }
        }

        for file_desc in fds {
            let mut flags = file_desc.fd_flags();
            flags.insert(FdFlags::CLOSE_ON_EXEC);
            file_desc.set_fd_flags(flags);
        }
    }

    pub(super) fn get_fd(&self, fd: Fd) -> Result<Arc<FileDesc>, SysError> {
        if fd.raw() as usize >= self.fds.len() {
            return Err(SysError::BadFileDescriptor);
        }

        if let Some(file_desc) = &self.fds[fd.raw() as usize] {
            Ok(file_desc.clone())
        } else {
            Err(SysError::BadFileDescriptor)
        }
    }

    pub fn opened_fd_numbers_snapshot(&self) -> Vec<Fd> {
        self.fds
            .iter()
            .enumerate()
            .filter_map(|(fd, file_desc)| {
                let opened = file_desc.is_some();
                let reserved = self.reserved_bitmap.test(fd);
                assert!(
                    self.bitmap.test(fd) == (opened || reserved),
                    "FilesState bitmap/fds open-state diverged"
                );
                assert!(
                    !(opened && reserved),
                    "FilesState slot cannot be both open and reserved"
                );

                opened.then(|| Fd::new(fd as u32).expect("fd table index must fit in Fd"))
            })
            .collect()
    }

    pub(super) fn dup(&mut self, old_fd: Fd) -> Result<Fd, SysError> {
        let file_desc = self.get_fd(old_fd)?;
        let fd = self.alloc()?;
        // note: new file desc, shared proc file.
        self.publish_fd_desc(
            fd,
            Arc::new(FileDesc::new_unpublished(
                file_desc.pfile.clone(),
                // Linux semantics: the new fd created by dup doesn't inherit the close-on-exec
                // flag of the old fd.
                FdFlags::empty(),
            )),
        );
        Ok(fd)
    }

    fn dup_ge_than(
        &mut self,
        old_fd: Fd,
        min_new_fd: Fd,
        close_on_exec: bool,
    ) -> Result<Fd, SysError> {
        let file_desc = self.get_fd(old_fd)?;
        let fd = self.alloc_ge_than(min_new_fd)?;
        let new_file_desc = Arc::new(FileDesc::new_unpublished(
            file_desc.pfile.clone(),
            if close_on_exec {
                FdFlags::CLOSE_ON_EXEC
            } else {
                FdFlags::empty()
            },
        ));
        self.publish_fd_desc(fd, new_file_desc);
        Ok(fd)
    }

    /// Linux's semantics of dup3 is a bit weird, currently we implement a
    /// reasonable subset of it. If in the future we get stuck with
    /// compatibility issues, we'll implement the rest of it.
    fn dup3(
        &mut self,
        old_fd: Fd,
        new_fd: Fd,
        flags: FdFlags,
    ) -> Result<Vec<Arc<ProcFile>>, SysError> {
        if new_fd.raw() as usize >= self.fds.len() {
            return Err(SysError::BadFileDescriptor);
        }

        if old_fd == new_fd {
            return Err(SysError::InvalidArgument);
        }

        let file_desc = self.get_fd(old_fd)?;
        let new_idx = new_fd.raw() as usize;
        let mut closed = Vec::new();

        if self.fds[new_idx].is_some() {
            closed.push(self.close_fd(new_fd)?);
        } else if self.bitmap.test(new_idx) {
            return Err(SysError::NoMoreFd);
        }

        self.alloc_at(new_fd)?;
        let new_file_desc = Arc::new(FileDesc::new_unpublished(file_desc.pfile.clone(), flags));
        self.publish_fd_desc(new_fd, new_file_desc);
        Ok(closed)
    }

    fn close_on_exec(&mut self) -> Vec<Arc<ProcFile>> {
        let mut closed = Vec::new();
        for fd in 0..self.fds.len() {
            if let Some(file_desc) = &self.fds[fd] {
                if file_desc.fd_flags().contains(FdFlags::CLOSE_ON_EXEC) {
                    let pfile = self.close_fd(Fd::new(fd as u32).unwrap()).expect(
                        "we've validated those created fds before, so they must be valid to close",
                    );
                    closed.push(pfile);
                }
            }
        }
        closed
    }

    fn drain_all_published_fds(&mut self) -> Vec<Arc<ProcFile>> {
        let mut closed = Vec::new();
        for (fd, file_desc) in self.fds.iter_mut().enumerate() {
            if let Some(file_desc) = file_desc.take() {
                assert!(
                    self.bitmap.test(fd),
                    "published fd slot missing allocator bit during explicit fd-table cleanup"
                );
                assert!(
                    !self.reserved_bitmap.test(fd),
                    "published fd slot marked reserved during explicit fd-table cleanup"
                );
                closed.push(file_desc.unpublish_from_fd_table());
            }
        }

        // This is an explicit lifetime boundary for a whole fd table. Reserved
        // slots are allocator state, not opened descriptions, so they are
        // cleared here instead of relying on `Drop` to repair leaked state.
        self.bitmap.clear_all();
        self.reserved_bitmap.clear_all();
        closed
    }

    pub fn fork(&self) -> Self {
        // note: we should clone file desc itself, not the arc, so that we can
        // have different fd flags for the new fd table.
        let mut bitmap = Bitmap::new();
        let fds = self
            .fds
            .iter()
            .enumerate()
            .map(|(fd_idx, fd_opt)| {
                fd_opt.as_ref().map(|file_desc| {
                    let new_fd = Arc::new(FileDesc::new_unpublished(
                        file_desc.pfile.clone(),
                        file_desc.fd_flags(),
                    ));
                    new_fd.publish_to_fd_table();
                    bitmap.set(fd_idx);
                    new_fd
                })
            })
            .collect();
        let reserved_bitmap = Bitmap::new();

        Self {
            bitmap,
            reserved_bitmap,
            fds,
        }
    }
}

impl Drop for FilesState {
    fn drop(&mut self) {
        assert!(
            self.fds.iter().all(Option::is_none),
            "FilesState dropped with published fd slots; missing explicit fd-table cleanup"
        );
        assert!(
            self.bitmap.is_empty(),
            "FilesState dropped with allocator bits set; missing explicit fd-table cleanup"
        );
        assert!(
            self.reserved_bitmap.is_empty(),
            "FilesState dropped with reserved fd slots; missing explicit fd-table cleanup"
        );
    }
}

impl Task {
    fn release_description_ref(pfile: Arc<ProcFile>) {
        pfile.release_description_ref();
    }

    fn release_description_refs(closed: Vec<Arc<ProcFile>>) {
        for pfile in closed {
            Self::release_description_ref(pfile);
        }
    }

    fn drain_files_state_handle_if_last_arc(files_state: Arc<RwLock<FilesState>>) {
        // A shared CLONE_FILES table has one set of published slots owned by the
        // still-shared table. Replacing this task's handle must not unpublish
        // those slots while another task can still observe them. The Arc count
        // is a conservative ownership proxy: count > 1 may include temporary
        // observers, but skipping semantic cleanup is preferable to closing a
        // table another task may still share.
        if Arc::strong_count(&files_state) == 1 {
            let closed = files_state.write().drain_all_published_fds();
            Self::release_description_refs(closed);
        }
    }

    pub fn open_fd(
        &self,
        file: File,
        access: OpenAccessMode,
        status_flags: FileStatusFlags,
        compat: LinuxOpenCompat,
        fd_flags: FdFlags,
    ) -> Result<Fd, SysError> {
        let files_state = self.files_state();
        let mut files_state = files_state.write();
        files_state.open_fd(file, access, status_flags, compat, fd_flags)
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
        let files_state = self.files_state();
        let mut files_state = files_state.write();
        files_state.open_fd_with_description_ops(
            file,
            access,
            status_flags,
            compat,
            fd_flags,
            description_ops,
        )
    }

    pub fn reserve_fd(&self) -> Result<FdReservation, SysError> {
        let files_state = self.files_state();
        let fd = files_state.write().reserve_fd()?;
        Ok(FdReservation {
            files_state,
            fd,
            active: true,
        })
    }

    pub fn get_fd(&self, fd: Fd) -> Result<Arc<FileDesc>, SysError> {
        let files_state = self.files_state();
        files_state.read().get_fd(fd)
    }

    pub fn opened_fd_numbers_snapshot(&self) -> Vec<Fd> {
        let files_state = self.files_state();
        files_state.read().opened_fd_numbers_snapshot()
    }

    pub fn files_state(&self) -> Arc<RwLock<FilesState>> {
        self.files_state.read().clone()
    }

    /// Replace the contents of the current file-table state object.
    ///
    /// If this task is sharing the same file-table handle with other tasks,
    /// they will observe the updated contents as well.
    ///
    /// Note the semantic difference between this function and
    /// [`Self::replace_files_state_handle`].
    pub fn set_files_state(&self, files_state: FilesState) {
        let files_state_handle = self.files_state();
        let mut old = {
            let mut guard = files_state_handle.write();
            core::mem::replace(&mut *guard, files_state)
        };
        let closed = old.drain_all_published_fds();
        Self::release_description_refs(closed);
        drop(old);
    }

    /// Replace the shared file-table state handle.
    ///
    /// This should only be used while the task is still uniquely owned, such
    /// as during task construction or clone setup.
    pub fn replace_files_state_handle(&mut self, files_state: Arc<RwLock<FilesState>>) {
        let old = {
            let mut guard = self.files_state.write();
            core::mem::replace(&mut *guard, files_state)
        };
        Self::drain_files_state_handle_if_last_arc(old);
    }

    pub fn close_all_fds_for_exit(&self) {
        assert!(
            IntrArch::local_intr_enabled(),
            "fd-table exit cleanup must run with interrupts enabled"
        );
        assert!(
            allow_preempt(),
            "fd-table exit cleanup must run in a sleepable context"
        );

        let old = {
            let mut guard = self.files_state.write();
            core::mem::replace(&mut *guard, Arc::new(RwLock::new(FilesState::new())))
        };
        Self::drain_files_state_handle_if_last_arc(old);
    }

    pub fn close_fd(&self, fd: Fd) -> Result<(), SysError> {
        let files_state = self.files_state();
        let pfile = {
            let mut files_state = files_state.write();
            files_state.close_fd(fd)?
        };
        Self::release_description_ref(pfile);
        Ok(())
    }

    pub fn dup(&self, old_fd: Fd) -> Result<Fd, SysError> {
        let files_state = self.files_state();
        let mut files_state = files_state.write();
        files_state.dup(old_fd)
    }

    pub fn dup_ge_than(
        &self,
        old_fd: Fd,
        min_new_fd: Fd,
        close_on_exec: bool,
    ) -> Result<Fd, SysError> {
        let files_state = self.files_state();
        let mut files_state = files_state.write();
        files_state.dup_ge_than(old_fd, min_new_fd, close_on_exec)
    }

    pub fn dup3(&self, old_fd: Fd, new_fd: Fd, flags: FdFlags) -> Result<Fd, SysError> {
        let files_state = self.files_state();
        let closed = {
            let mut files_state = files_state.write();
            files_state.dup3(old_fd, new_fd, flags)?
        };
        Self::release_description_refs(closed);
        Ok(new_fd)
    }

    pub fn close_cloexec_fds(&self) {
        let files_state = self.files_state();
        let closed = files_state.write().close_on_exec();
        Self::release_description_refs(closed);
    }

    pub fn unshare_files_state(&self) {
        let forked = {
            let files_state = self.files_state();
            Arc::new(RwLock::new(files_state.read().fork()))
        };

        let old = {
            let mut guard = self.files_state.write();
            core::mem::replace(&mut *guard, forked)
        };
        Self::drain_files_state_handle_if_last_arc(old);
    }

    pub fn close_range(
        &self,
        first: u32,
        last: u32,
        flags: crate::fs::api::close::CloseRangeFlags,
    ) {
        if flags.contains(crate::fs::api::close::CloseRangeFlags::UNSHARE) {
            self.unshare_files_state();
        }

        let files_state = self.files_state();
        if flags.contains(crate::fs::api::close::CloseRangeFlags::CLOEXEC) {
            files_state.read().set_close_on_exec_range(first, last);
        } else {
            let closed = files_state.write().close_range(first, last);
            Self::release_description_refs(closed);
        }
    }
}
