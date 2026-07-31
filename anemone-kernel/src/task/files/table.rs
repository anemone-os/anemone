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
pub(super) struct FileTable {
    // `bitmap` is the allocator truth source: a set bit means the slot is
    // either published or reserved. `reserved_bitmap` marks the unpublished
    // subset. Published slots are the only ones visible through `fds`.
    bitmap: Bitmap<{ MAX_FD_PER_PROCESS / 64 }>,
    reserved_bitmap: Bitmap<{ MAX_FD_PER_PROCESS / 64 }>,
    fds: Vec<Option<Arc<FileDesc>>>,
}
// fd alloc
impl FileTable {
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

    pub(super) fn reserve_fd(&mut self) -> Result<Fd, SysError> {
        let fd = self.alloc()?;
        let idx = fd.raw() as usize;
        assert!(self.fds[idx].is_none());
        assert!(!self.reserved_bitmap.test(idx));
        self.reserved_bitmap.set(idx);
        Ok(fd)
    }

    pub(super) fn commit_reserved_fd(&mut self, fd: Fd, file_desc: Arc<FileDesc>) {
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

    pub(super) fn rollback_reserved_fd(&mut self, fd: Fd) {
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
impl FileTable {
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

    pub(super) fn open_fd_with_description_ops(
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

    pub(super) fn close_range(&mut self, first: u32, last: u32) -> Vec<Arc<ProcFile>> {
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

    pub(super) fn set_close_on_exec_range(&self, first: u32, last: u32) {
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
                    "FileTable bitmap/fds open-state diverged"
                );
                assert!(
                    !(opened && reserved),
                    "FileTable slot cannot be both open and reserved"
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

    pub(super) fn dup_ge_than(
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
    pub(super) fn dup3(
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

    pub(super) fn close_on_exec(&mut self) -> Vec<Arc<ProcFile>> {
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

    pub(super) fn drain_all_published_fds(&mut self) -> Vec<Arc<ProcFile>> {
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

    pub(super) fn fork(&self) -> Self {
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

impl Drop for FileTable {
    fn drop(&mut self) {
        assert!(
            self.fds.iter().all(Option::is_none),
            "FileTable dropped with published fd slots; missing explicit fd-table cleanup"
        );
        assert!(
            self.bitmap.is_empty(),
            "FileTable dropped with allocator bits set; missing explicit fd-table cleanup"
        );
        assert!(
            self.reserved_bitmap.is_empty(),
            "FileTable dropped with reserved fd slots; missing explicit fd-table cleanup"
        );
    }
}
