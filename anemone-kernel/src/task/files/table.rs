use crate::{
    prelude::{handler::TryFromSyscallArg, *},
    utils::bitmap::Bitmap,
};

use super::{
    descriptor::{FdFlags, FileDesc, FileStatusFlags, LinuxOpenCompat, OpenAccessMode},
    opened_description::FileDescOps,
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

/// Caller-specific fd-number cutoff for one allocation operation.
///
/// The resource-policy owner validates and snapshots this value. `FileTable`
/// consumes it without retaining a process or rlimit reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FdAllocCeiling(usize);

impl FdAllocCeiling {
    pub(crate) const fn new(ceiling: usize) -> Option<Self> {
        if ceiling <= MAX_FD_PER_PROCESS {
            Some(Self(ceiling))
        } else {
            None
        }
    }

    pub(crate) const fn contains(self, fd: Fd) -> bool {
        (fd.raw() as usize) < self.0
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
    fn alloc(&mut self, ceiling: FdAllocCeiling) -> Result<Fd, SysError> {
        if let Some(fd_idx) = self.bitmap.find_first_zero().filter(|fd| *fd < ceiling.0) {
            self.bitmap.set(fd_idx);
            let fd = Fd::new(fd_idx as u32).unwrap();
            assert!(self.fds[fd_idx].is_none());
            Ok(fd)
        } else {
            Err(SysError::NoMoreFd)
        }
    }

    fn alloc_ge_than(&mut self, min_fd: Fd, ceiling: FdAllocCeiling) -> Result<Fd, SysError> {
        if !ceiling.contains(min_fd) {
            return Err(SysError::InvalidArgument);
        }

        if let Some(fd_idx) = self
            .bitmap
            .find_first_zero_from(min_fd.raw() as usize)
            .filter(|fd| *fd < ceiling.0)
        {
            self.bitmap.set(fd_idx);
            let fd = Fd::new(fd_idx as u32).unwrap();
            assert!(self.fds[fd_idx].is_none());
            Ok(fd)
        } else {
            Err(SysError::NoMoreFd)
        }
    }

    fn alloc_at(&mut self, fd: Fd, ceiling: FdAllocCeiling) -> Result<(), SysError> {
        if !ceiling.contains(fd) {
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

    fn recycle(&mut self, fd: Fd) -> Arc<FileDesc> {
        debug_assert!(fd < Fd(MAX_FD_PER_PROCESS as u32));
        debug_assert!(self.fds[fd.raw() as usize].is_some());
        let file_desc = self.fds[fd.raw() as usize].take().unwrap();
        self.bitmap.clear(fd.raw() as usize);
        file_desc.unpublish_from_fd_table();
        file_desc
    }

    pub(super) fn reserve_fd(&mut self, ceiling: FdAllocCeiling) -> Result<Fd, SysError> {
        let fd = self.alloc(ceiling)?;
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
        ceiling: FdAllocCeiling,
        file: File,
        access: OpenAccessMode,
        status_flags: FileStatusFlags,
        compat: LinuxOpenCompat,
        fd_flags: FdFlags,
    ) -> Result<Fd, SysError> {
        self.open_fd_with_description_ops(
            ceiling,
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
        ceiling: FdAllocCeiling,
        file: File,
        access: OpenAccessMode,
        status_flags: FileStatusFlags,
        compat: LinuxOpenCompat,
        fd_flags: FdFlags,
        description_ops: FileDescOps,
    ) -> Result<Fd, SysError> {
        let fd = self.alloc(ceiling)?;
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

    pub(super) fn close_fd(&mut self, fd: Fd) -> Result<Arc<FileDesc>, SysError> {
        if fd.raw() as usize >= self.fds.len() {
            return Err(SysError::BadFileDescriptor);
        }

        if self.fds[fd.raw() as usize].is_some() {
            Ok(self.recycle(fd))
        } else {
            Err(SysError::BadFileDescriptor)
        }
    }

    pub(super) fn close_range(&mut self, first: u32, last: u32) -> Vec<Arc<FileDesc>> {
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

    pub(super) fn dup(&mut self, old_fd: Fd, ceiling: FdAllocCeiling) -> Result<Fd, SysError> {
        let file_desc = self.get_fd(old_fd)?;
        let fd = self.alloc(ceiling)?;
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
        ceiling: FdAllocCeiling,
    ) -> Result<Fd, SysError> {
        let file_desc = self.get_fd(old_fd)?;
        let fd = self.alloc_ge_than(min_new_fd, ceiling)?;
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
        ceiling: FdAllocCeiling,
    ) -> Result<Vec<Arc<FileDesc>>, SysError> {
        if new_fd.raw() as usize >= self.fds.len() {
            return Err(SysError::BadFileDescriptor);
        }

        if old_fd == new_fd {
            return Err(SysError::InvalidArgument);
        }

        // Reject the caller-specific cutoff before withdrawing an existing
        // target slot or running opened-description/POSIX-lock cleanup.
        if !ceiling.contains(new_fd) {
            return Err(SysError::BadFileDescriptor);
        }

        let file_desc = self.get_fd(old_fd)?;
        let new_idx = new_fd.raw() as usize;
        let mut closed = Vec::new();

        if self.fds[new_idx].is_some() {
            closed.push(self.close_fd(new_fd)?);
        } else if self.bitmap.test(new_idx) {
            return Err(SysError::NoMoreFd);
        }

        self.alloc_at(new_fd, ceiling)?;
        let new_file_desc = Arc::new(FileDesc::new_unpublished(file_desc.pfile.clone(), flags));
        self.publish_fd_desc(new_fd, new_file_desc);
        Ok(closed)
    }

    pub(super) fn close_on_exec(&mut self) -> Vec<Arc<FileDesc>> {
        let mut closed = Vec::new();
        for fd in 0..self.fds.len() {
            if let Some(file_desc) = &self.fds[fd] {
                if file_desc.fd_flags().contains(FdFlags::CLOSE_ON_EXEC) {
                    let file_desc = self.close_fd(Fd::new(fd as u32).unwrap()).expect(
                        "we've validated those created fds before, so they must be valid to close",
                    );
                    closed.push(file_desc);
                }
            }
        }
        closed
    }

    pub(super) fn drain_all_published_fds(&mut self) -> Vec<Arc<FileDesc>> {
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
                file_desc.unpublish_from_fd_table();
                closed.push(file_desc);
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

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn ceiling(value: usize) -> FdAllocCeiling {
        FdAllocCeiling::new(value).unwrap()
    }

    fn root_file() -> File {
        vfs_open(Path::new("/")).unwrap()
    }

    fn open_root(table: &mut FileTable, ceiling: FdAllocCeiling) -> Fd {
        table
            .open_fd(
                ceiling,
                root_file(),
                OpenAccessMode::Read,
                FileStatusFlags::empty(),
                LinuxOpenCompat::empty(),
                FdFlags::empty(),
            )
            .unwrap()
    }

    fn release_all(table: &mut FileTable) {
        for file_desc in table.drain_all_published_fds() {
            file_desc.release_description_ref();
        }
    }

    #[kunit]
    fn allocation_range_honors_zero_boundary_and_full_lower_range() {
        let mut table = FileTable::new();
        assert_eq!(table.reserve_fd(ceiling(0)), Err(SysError::NoMoreFd));

        let first = table.reserve_fd(ceiling(2)).unwrap();
        let second = table.reserve_fd(ceiling(2)).unwrap();
        assert_eq!(first, Fd::new(0).unwrap());
        assert_eq!(second, Fd::new(1).unwrap());
        assert_eq!(table.reserve_fd(ceiling(2)), Err(SysError::NoMoreFd));
        assert!(!table.bitmap.test(2));

        table.rollback_reserved_fd(first);
        assert_eq!(table.reserve_fd(ceiling(2)).unwrap(), first);
        table.rollback_reserved_fd(first);
        table.rollback_reserved_fd(second);
        assert!(table.bitmap.is_empty());
        assert!(table.reserved_bitmap.is_empty());
    }

    #[kunit]
    fn open_reserve_dup_and_target_dup_share_one_cutoff() {
        let mut table = FileTable::new();
        let original = open_root(&mut table, ceiling(2));
        let reserved = table.reserve_fd(ceiling(2)).unwrap();
        assert_eq!(table.reserve_fd(ceiling(2)), Err(SysError::NoMoreFd));
        table.rollback_reserved_fd(reserved);

        let duplicate = table.dup(original, ceiling(2)).unwrap();
        assert_eq!(duplicate, Fd::new(1).unwrap());
        let removed = table.close_fd(duplicate).unwrap();
        removed.release_description_ref();

        let minimum = Fd::new(3).unwrap();
        let high = table
            .dup_ge_than(original, minimum, false, ceiling(4))
            .unwrap();
        assert_eq!(high, minimum);
        assert_eq!(
            table.dup_ge_than(original, Fd::new(4).unwrap(), false, ceiling(4)),
            Err(SysError::InvalidArgument)
        );

        table
            .dup3(original, Fd::new(5).unwrap(), FdFlags::empty(), ceiling(6))
            .unwrap();
        let target_before = table.get_fd(Fd::new(5).unwrap()).unwrap();
        assert!(matches!(
            table.dup3(original, Fd::new(5).unwrap(), FdFlags::empty(), ceiling(5)),
            Err(SysError::BadFileDescriptor)
        ));
        let target_after = table.get_fd(Fd::new(5).unwrap()).unwrap();
        assert!(Arc::ptr_eq(&target_before, &target_after));

        // Lowering never retracts already published descriptors.
        assert!(table.get_fd(high).is_ok());
        assert!(table.get_fd(Fd::new(5).unwrap()).is_ok());
        release_all(&mut table);
    }

    #[kunit]
    fn reservation_commit_uses_the_existing_slot_claim() {
        let mut table = FileTable::new();
        let fd = table.reserve_fd(ceiling(1)).unwrap();
        let description = FileDesc::new_opened(
            root_file(),
            OpenAccessMode::Read,
            FileStatusFlags::empty(),
            LinuxOpenCompat::empty(),
            FdFlags::empty(),
            FileDescOps::default(),
        );

        // Commit consumes the reservation; it does not accept a new policy
        // input or perform a second allocation after a concurrent lowering.
        table.commit_reserved_fd(fd, description);
        assert!(table.get_fd(fd).is_ok());
        assert!(!table.reserved_bitmap.test(fd.raw() as usize));
        assert_eq!(table.reserve_fd(ceiling(1)), Err(SysError::NoMoreFd));
        release_all(&mut table);
    }
}
