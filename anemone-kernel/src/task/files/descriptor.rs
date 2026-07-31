use crate::{
    fs::{
        FcntlAccess, FcntlCtx, FileFcntlCmd, FileIoCtx, FileOpStatusFlags, PollRegisterResult,
        PollRequest, UserBufferSink, UserBufferSource,
    },
    prelude::*,
};

use super::opened_description::{
    FileDescOps, OpenedDescriptionCapability, OpenedFileReadUserCtx, ProcFile,
};

#[derive(Debug)]
pub struct FileDesc {
    pub(super) pfile: Arc<ProcFile>,
    // atomic integer may be better.
    flags: SpinLock<FdFlags>,
    /// True only while this descriptor object occupies a visible fd-table slot.
    published: AtomicBool,
}

impl Clone for FileDesc {
    fn clone(&self) -> Self {
        Self::new_unpublished(self.pfile.clone(), self.fd_flags())
    }
}

fn update_status_flags(
    status_flags: &SpinLock<FileStatusFlags>,
    update: impl Fn(FileStatusFlags) -> FileStatusFlags,
    mut validate: impl FnMut(FileStatusFlags) -> Result<(), SysError>,
) -> Result<(), SysError> {
    loop {
        let observed = *status_flags.lock();
        let candidate = update(observed);

        // Backend validation must remain outside the opened-description lock:
        // FileOps may inspect its owning object and must not inherit a task/files
        // lock-order dependency. Recheck the snapshot before committing so a
        // concurrent F_SETFL or FIONBIO update cannot be overwritten.
        validate(candidate)?;

        let mut current = status_flags.lock();
        if *current != observed {
            continue;
        }
        *current = candidate;
        return Ok(());
    }
}

// re-export FileOps here, with permission checked.
//
// TODO: we only checked permission of fd, but we haven't checked permission of
// the file itself.
impl FileDesc {
    pub(super) fn new_unpublished(pfile: Arc<ProcFile>, fd_flags: FdFlags) -> Self {
        Self {
            pfile,
            flags: SpinLock::new(fd_flags),
            published: AtomicBool::new(false),
        }
    }

    pub fn new_opened(
        file: File,
        access: OpenAccessMode,
        status_flags: FileStatusFlags,
        compat: LinuxOpenCompat,
        fd_flags: FdFlags,
        description_ops: FileDescOps,
    ) -> Arc<Self> {
        Arc::new(Self::new_unpublished(
            Arc::new(ProcFile::new(
                file,
                access,
                status_flags,
                compat,
                description_ops,
            )),
            fd_flags,
        ))
    }

    pub(super) fn publish_to_fd_table(&self) {
        let already_published = self.published.swap(true, Ordering::AcqRel);
        assert!(
            !already_published,
            "file description published into multiple fd table slots"
        );
        self.pfile.acquire_description_ref();
    }

    pub(super) fn unpublish_from_fd_table(&self) -> Arc<ProcFile> {
        let was_published = self.published.swap(false, Ordering::AcqRel);
        assert!(was_published, "unpublishing unpublished file descriptor");
        self.pfile.clone()
    }

    /// Capture this opened-description identity only while it is live.
    ///
    /// A concurrent final close may retire it immediately after this check;
    /// callers therefore use the capability's lease and commit-time recheck
    /// rather than treating successful capture as an alive bit.
    pub(crate) fn opened_description_capability(&self) -> Option<OpenedDescriptionCapability> {
        self.pfile
            .description_is_live()
            .then(|| OpenedDescriptionCapability {
                target: Arc::downgrade(&self.pfile),
            })
    }

    pub fn vfs_file(&self) -> &Arc<File> {
        &self.pfile.file
    }

    pub fn access_mode(&self) -> OpenAccessMode {
        self.pfile.access
    }

    pub fn can_read(&self) -> bool {
        self.pfile.access.can_read()
    }

    pub fn can_write(&self) -> bool {
        self.pfile.access.can_write()
    }

    pub fn is_path_only(&self) -> bool {
        self.pfile.access.is_path_only()
    }

    pub fn file_flags(&self) -> FileStatusFlags {
        *self.pfile.status_flags.lock()
    }

    pub fn ioctl_access(&self) -> IoctlFileAccess {
        let flags = self.file_flags();
        IoctlFileAccess::new(
            self.can_read(),
            self.can_write(),
            self.is_path_only(),
            flags.to_file_op_status_flags(),
        )
    }

    pub fn fcntl_ctx(&self, cmd: FileFcntlCmd, arg: u64) -> Result<FcntlCtx, SysError> {
        if self.is_path_only() {
            return Err(SysError::BadFileDescriptor);
        }

        let flags = self.file_flags();
        let access = FcntlAccess::new(
            self.can_read(),
            self.can_write(),
            flags.to_file_op_status_flags(),
        );
        Ok(FcntlCtx::new(cmd, arg, access))
    }

    fn update_file_flags(
        &self,
        update: impl Fn(FileStatusFlags) -> FileStatusFlags,
    ) -> Result<(), SysError> {
        update_status_flags(&self.pfile.status_flags, update, |candidate| {
            self.pfile
                .file
                .check_status_flags(candidate.to_file_op_status_flags())
        })
    }

    pub fn replace_settable_file_flags(&self, settable: FileStatusFlags) -> Result<(), SysError> {
        let allowed = FileStatusFlags::APPEND | FileStatusFlags::NONBLOCK | FileStatusFlags::DIRECT;
        assert!(
            (settable - allowed).is_empty(),
            "non-settable opened-description flags reached F_SETFL commit"
        );

        self.update_file_flags(|mut flags| {
            flags.set(
                FileStatusFlags::APPEND,
                settable.contains(FileStatusFlags::APPEND),
            );
            flags.set(
                FileStatusFlags::NONBLOCK,
                settable.contains(FileStatusFlags::NONBLOCK),
            );
            flags.set(
                FileStatusFlags::DIRECT,
                settable.contains(FileStatusFlags::DIRECT),
            );
            flags
        })
    }

    pub fn set_nonblocking(&self, enabled: bool) -> Result<(), SysError> {
        self.update_file_flags(|mut flags| {
            flags.set(FileStatusFlags::NONBLOCK, enabled);
            flags
        })
    }

    pub fn to_linux_getfl_flags(&self) -> u32 {
        self.pfile.access.to_linux_open_flags()
            | self.file_flags().to_linux_open_flags()
            | self.pfile.compat.getfl_visible_flags()
    }

    pub fn fd_flags(&self) -> FdFlags {
        *self.flags.lock()
    }

    pub fn set_fd_flags(&self, flags: FdFlags) {
        *self.flags.lock() = flags;
    }

    pub fn notifications_suppressed(&self) -> bool {
        self.pfile.description_ops.notification_suppressed
    }

    pub fn notify_read_user_access(&self) -> bool {
        self.pfile.description_ops.notify_read_user_access
    }

    pub fn read_user_transaction(
        &self,
        dst: &mut UserBufferSink<'_>,
    ) -> Option<Result<usize, SysError>> {
        if !self.can_read() {
            return Some(Err(SysError::BadFileDescriptor));
        }

        let read_user_transaction = self.pfile.description_ops.read_user_transaction?;
        Some(read_user_transaction(OpenedFileReadUserCtx {
            file: self.pfile.file.as_ref(),
            status_flags: self.file_flags(),
            dst,
            notification_suppressed: self.notifications_suppressed(),
        }))
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, SysError> {
        if !self.can_read() {
            return Err(SysError::BadFileDescriptor);
        }
        let ctx = FileIoCtx::new(self.file_flags().to_file_op_status_flags());
        self.pfile
            .file
            .read_with_ctx(buf, ctx)
            .map_err(|e| e.into())
    }

    pub(crate) fn read_user(
        &self,
        dst: &mut UserBufferSink<'_>,
    ) -> Option<Result<usize, SysError>> {
        let file = self.pfile.file.as_ref();
        if !file.has_read_user_at() {
            return None;
        }
        if !self.can_read() {
            return Some(Err(SysError::BadFileDescriptor));
        }

        let ctx = FileIoCtx::new(self.file_flags().to_file_op_status_flags());
        Some(file.read_user_with_ctx(dst, ctx).map_err(|e| e.into()))
    }

    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize, SysError> {
        if !self.can_read() {
            return Err(SysError::BadFileDescriptor);
        }
        if self.is_path_only() {
            return Err(SysError::BadFileDescriptor);
        }
        let ctx = FileIoCtx::new(self.file_flags().to_file_op_status_flags());
        self.pfile
            .file
            .read_at_with_ctx(offset, buf, ctx)
            .map_err(|e| e.into())
    }

    pub(crate) fn read_user_at(
        &self,
        offset: usize,
        dst: &mut UserBufferSink<'_>,
    ) -> Option<Result<usize, SysError>> {
        let file = self.pfile.file.as_ref();
        if !file.has_read_user_at() {
            return None;
        }
        if !self.can_read() || self.is_path_only() {
            return Some(Err(SysError::BadFileDescriptor));
        }

        let ctx = FileIoCtx::new(self.file_flags().to_file_op_status_flags());
        Some(
            file.read_user_at_with_ctx(offset, dst, ctx)
                .map_err(|e| e.into()),
        )
    }

    /// This applies to both write and append mode.
    pub fn write(&self, buf: &[u8]) -> Result<usize, SysError> {
        let flags = self.file_flags();
        if !self.can_write() {
            return Err(SysError::BadFileDescriptor);
        }

        let ctx = FileIoCtx::new(flags.to_file_op_status_flags());
        let file = self.pfile.file.as_ref();
        if file.is_stream() {
            return file.write_with_ctx(buf, ctx).map_err(|e| e.into());
        }

        if flags.contains(FileStatusFlags::APPEND) {
            file.append_with_ctx(buf, ctx).map_err(|e| e.into())
        } else {
            file.write_with_ctx(buf, ctx).map_err(|e| e.into())
        }
    }

    /// Positioned writes keep the file cursor unchanged.
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> Result<usize, SysError> {
        let flags = self.file_flags();
        if !self.can_write() {
            return Err(SysError::BadFileDescriptor);
        }
        if self.is_path_only() {
            return Err(SysError::BadFileDescriptor);
        }
        let ctx = FileIoCtx::new(flags.to_file_op_status_flags());
        let file = self.pfile.file.as_ref();
        if file.is_stream() {
            return file
                .write_at_with_ctx(offset, buf, ctx)
                .map_err(|e| e.into());
        }

        if flags.contains(FileStatusFlags::APPEND) {
            return file
                .append_at_current_end_with_ctx(buf, ctx)
                .map_err(|e| e.into());
        }
        file.write_at_with_ctx(offset, buf, ctx)
            .map_err(|e| e.into())
    }

    pub(crate) fn write_user(
        &self,
        src: &mut UserBufferSource<'_>,
    ) -> Option<Result<usize, SysError>> {
        let file = self.pfile.file.as_ref();
        if !file.has_write_user_at() {
            return None;
        }

        let flags = self.file_flags();
        if !self.can_write() {
            return Some(Err(SysError::BadFileDescriptor));
        }
        if file.is_stream() {
            return Some(Err(SysError::BadFileDescriptor));
        }

        let ctx = FileIoCtx::new(flags.to_file_op_status_flags());
        Some(if flags.contains(FileStatusFlags::APPEND) {
            file.append_user_with_ctx(src, ctx).map_err(|e| e.into())
        } else {
            file.write_user_with_ctx(src, ctx).map_err(|e| e.into())
        })
    }

    pub(crate) fn write_user_at(
        &self,
        offset: usize,
        src: &mut UserBufferSource<'_>,
    ) -> Option<Result<usize, SysError>> {
        let file = self.pfile.file.as_ref();
        if !file.has_write_user_at() {
            return None;
        }

        let flags = self.file_flags();
        if !self.can_write() || self.is_path_only() {
            return Some(Err(SysError::BadFileDescriptor));
        }
        if file.is_stream() {
            return Some(Err(SysError::BadFileDescriptor));
        }

        let ctx = FileIoCtx::new(flags.to_file_op_status_flags());
        Some(if flags.contains(FileStatusFlags::APPEND) {
            file.append_user_at_current_end_with_ctx(src, ctx)
                .map_err(|e| e.into())
        } else {
            file.write_user_at_with_ctx(offset, src, ctx)
                .map_err(|e| e.into())
        })
    }

    pub fn truncate(&self, len: u64, cred: &CredentialSet) -> Result<(), SysError> {
        if !self.can_write() {
            return Err(SysError::BadFileDescriptor);
        }

        let inode = self.pfile.file.inode();
        if inode.ty() == InodeType::Regular {
            self.pfile.file.path().mount().ensure_writable()?;
        }

        inode.truncate(len, cred)
    }

    /// Linux whence values are converted in syscall handlers; FileDesc only
    /// forwards the internal seek intent.
    pub fn seek(&self, from: SeekFrom) -> Result<usize, SysError> {
        if self.is_path_only() {
            return Err(SysError::BadFileDescriptor);
        }
        self.pfile.file.seek(from).map_err(|e| e.into())
    }

    pub fn read_dir(&self, sink: &mut dyn DirSink) -> Result<ReadDirResult, SysError> {
        if self.is_path_only() {
            return Err(SysError::BadFileDescriptor);
        }
        self.pfile.file.read_dir(sink).map_err(|e| e.into())
    }

    pub fn poll(&self, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
        self.pfile.file.poll(request).map_err(|e| e.into())
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FileStatusFlags: u32 {
        const APPEND = 0b0001;
        const NONBLOCK = 0b0010;
        const DIRECT = 0b0100;
        const DSYNC = 0b1000;
        const SYNC = 0b1_0000;
        const NOATIME = 0b10_0000;

        // create, truncate, and fd-local close-on-exec are not persistent file
        // status flags, so they don't live in the shared ProcFile status state.
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAccessMode {
    Read,
    Write,
    ReadWrite,
    Path,
}

impl OpenAccessMode {
    pub const fn can_read(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }

    pub const fn can_write(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite)
    }

    pub const fn is_path_only(self) -> bool {
        matches!(self, Self::Path)
    }

    pub fn to_linux_open_flags(self) -> u32 {
        use anemone_abi::fs::linux::open::*;

        match self {
            Self::Read => O_RDONLY,
            Self::Write => O_WRONLY,
            Self::ReadWrite => O_RDWR,
            Self::Path => O_PATH,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinuxOpenCompat {
    getfl_visible_flags: u32,
    accepted_noop_flags: u32,
}

impl LinuxOpenCompat {
    pub const fn new(getfl_visible_flags: u32, accepted_noop_flags: u32) -> Self {
        Self {
            getfl_visible_flags,
            accepted_noop_flags,
        }
    }

    pub const fn empty() -> Self {
        Self::new(0, 0)
    }

    pub const fn getfl_visible_flags(self) -> u32 {
        self.getfl_visible_flags
    }

    pub const fn accepted_noop_flags(self) -> u32 {
        self.accepted_noop_flags
    }
}

impl FileStatusFlags {
    /// Normalized short-lived snapshot passed to FileOps contexts. The opened
    /// file description remains the only owner of mutable status flags.
    pub fn to_file_op_status_flags(self) -> FileOpStatusFlags {
        let mut flags = FileOpStatusFlags::empty();
        flags.set(
            FileOpStatusFlags::APPEND,
            self.contains(FileStatusFlags::APPEND),
        );
        flags.set(
            FileOpStatusFlags::NONBLOCK,
            self.contains(FileStatusFlags::NONBLOCK),
        );
        flags.set(
            FileOpStatusFlags::DIRECT,
            self.contains(FileStatusFlags::DIRECT),
        );
        flags.set(
            FileOpStatusFlags::DSYNC,
            self.contains(FileStatusFlags::DSYNC),
        );
        flags.set(
            FileOpStatusFlags::SYNC,
            self.contains(FileStatusFlags::SYNC),
        );
        flags.set(
            FileOpStatusFlags::NOATIME,
            self.contains(FileStatusFlags::NOATIME),
        );
        flags
    }

    pub fn to_linux_open_flags(&self) -> u32 {
        use anemone_abi::fs::linux::open::*;

        let mut flags = 0;

        if self.contains(Self::APPEND) {
            flags |= O_APPEND;
        }
        if self.contains(Self::NONBLOCK) {
            flags |= O_NONBLOCK;
        }
        if self.contains(Self::DIRECT) {
            flags |= O_DIRECT;
        }
        if self.contains(Self::DSYNC) {
            flags |= O_DSYNC;
        }
        if self.contains(Self::SYNC) {
            flags |= O_SYNC;
        }
        if self.contains(Self::NOATIME) {
            flags |= O_NOATIME;
        }

        flags
    }

    pub fn settable_from_linux_flags(raw: u32) -> Self {
        use anemone_abi::fs::linux::open::*;

        let mut flags = Self::empty();
        flags.set(Self::APPEND, raw & O_APPEND != 0);
        flags.set(Self::NONBLOCK, raw & O_NONBLOCK != 0);
        flags.set(Self::DIRECT, raw & O_DIRECT != 0);
        flags
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FdFlags: u32 {
        /// If set, the file descriptor will be automatically closed when executing
        /// a new program.
        ///
        /// Hmm... it seems that O_CLOEXEC is the only FdFlag?
        const CLOSE_ON_EXEC = 0b0001;
    }
}

impl FdFlags {
    pub fn from_linux_open_flags(flags: u32) -> Self {
        let mut fd_flags = Self::empty();
        if flags & anemone_abi::fs::linux::open::O_CLOEXEC != 0 {
            fd_flags |= Self::CLOSE_ON_EXEC;
        }

        fd_flags
    }
}

static_assert!(
    MAX_FD_PER_PROCESS.is_multiple_of(64),
    "to fit well with bitmap"
);

#[cfg(feature = "kunit")]
mod status_flag_kunits {
    use core::cell::Cell;

    use super::*;

    #[kunit]
    fn rejected_status_update_leaves_opened_description_unchanged() {
        let status = SpinLock::new(FileStatusFlags::APPEND);

        let error = update_status_flags(
            &status,
            |mut flags| {
                flags.insert(FileStatusFlags::NONBLOCK);
                flags
            },
            |_| Err(SysError::InvalidArgument),
        )
        .unwrap_err();

        assert_eq!(error, SysError::InvalidArgument);
        assert_eq!(*status.lock(), FileStatusFlags::APPEND);
    }

    #[kunit]
    fn status_update_retries_after_concurrent_commit() {
        let status = SpinLock::new(FileStatusFlags::APPEND);
        let validation_count = Cell::new(0usize);

        update_status_flags(
            &status,
            |mut flags| {
                flags.insert(FileStatusFlags::NONBLOCK);
                flags
            },
            |_| {
                let count = validation_count.get();
                validation_count.set(count + 1);
                if count == 0 {
                    // Model another status writer committing after our snapshot
                    // but before our compare-and-commit step.
                    *status.lock() = FileStatusFlags::DIRECT;
                }
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(validation_count.get(), 2);
        assert_eq!(
            *status.lock(),
            FileStatusFlags::DIRECT | FileStatusFlags::NONBLOCK
        );
    }
}
