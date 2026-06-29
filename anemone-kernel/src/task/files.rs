//! File descriptor management for a task.
//!
//! Reference:
//! - https://elixir.bootlin.com/linux/v6.6.32/source/include/linux/fdtable.h

use crate::{
    fs::{FcntlAccess, FcntlCtx, FileFcntlCmd, FileIoCtx, FileOpStatusFlags, UserBufferSink},
    prelude::{handler::TryFromSyscallArg, *},
    utils::bitmap::Bitmap,
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

/// Shared VFS opened handle.
///
/// This object is not process-local. Duplicated descriptors and forked file
/// tables share it, including file status flags and the opened file handle.
#[derive(Debug)]
struct ProcFile {
    /// Vfs file handle. Task-agnostic.
    file: Arc<File>,
    access: OpenAccessMode,
    status_flags: SpinLock<FileStatusFlags>,
    compat: LinuxOpenCompat,
    /// Counts published fd-table slots, not transient `Arc<FileDesc>` borrows.
    ///
    /// Final-release callbacks use this as the opened-file-description lifetime
    /// boundary, so syscall-local clones from `get_fd()` cannot keep semantic
    /// close teardown from running.
    description_refs: AtomicUsize,
    description_ops: FileDescOps,
}

#[derive(Debug)]
pub struct FileDesc {
    pfile: Arc<ProcFile>,
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

/// Rare hooks attached to an opened file description.
///
/// This is not a backend vtable like `FileOps`: most files use the default
/// empty hooks. Add entries here only for behavior that depends on the opened
/// description or fd-facing syscall transaction, such as direct userspace
/// copyout, final published-fd release, or generic notification suppression.
#[derive(Clone, Copy)]
pub struct FileDescOps {
    /// Optional opened-description read transaction for files whose read
    /// operation cannot be modeled as kernel-buffer fill followed by generic
    /// copyout. This is not an ordinary filesystem direct-user fast path.
    pub read_user_transaction:
        Option<for<'dst, 'buf> fn(OpenedFileReadUserCtx<'dst, 'buf>) -> Result<usize, SysError>>,
    /// Whether successful direct read-user dispatch is an ordinary access
    /// event source. Protocol/control fds can use read_user_transaction for
    /// copyout while remaining outside file-content access notification.
    pub notify_read_user_access: bool,
    /// Runs when the last published fd-table slot for this opened file
    /// description is removed. Transient syscall refs do not delay it.
    pub final_release: Option<for<'a> fn(OpenedFileFinalReleaseCtx<'a>)>,
    /// Generic kernel-only event suppression marker. VFS hooks may inspect this
    /// capability, but task/fd code must not attach feature-specific meaning.
    pub notification_suppressed: bool,
}

impl Default for FileDescOps {
    fn default() -> Self {
        Self {
            read_user_transaction: None,
            notify_read_user_access: true,
            final_release: None,
            notification_suppressed: false,
        }
    }
}

impl core::fmt::Debug for FileDescOps {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FileDescOps")
            .field(
                "read_user_transaction",
                &self.read_user_transaction.is_some(),
            )
            .field("notify_read_user_access", &self.notify_read_user_access)
            .field("final_release", &self.final_release.is_some())
            .field("notification_suppressed", &self.notification_suppressed)
            .finish()
    }
}

pub struct OpenedFileReadUserCtx<'ctx, 'buf> {
    pub file: &'ctx File,
    pub status_flags: FileStatusFlags,
    pub dst: &'ctx mut UserBufferSink<'buf>,
    pub notification_suppressed: bool,
}

pub struct OpenedFileFinalReleaseCtx<'a> {
    pub file: &'a File,
    pub access: OpenAccessMode,
    pub notification_suppressed: bool,
}

impl ProcFile {
    fn new(
        file: File,
        access: OpenAccessMode,
        status_flags: FileStatusFlags,
        compat: LinuxOpenCompat,
        description_ops: FileDescOps,
    ) -> Self {
        Self {
            file: Arc::new(file),
            access,
            status_flags: SpinLock::new(status_flags),
            compat,
            description_refs: AtomicUsize::new(0),
            description_ops,
        }
    }

    fn acquire_description_ref(&self) {
        let prev = self.description_refs.fetch_add(1, Ordering::AcqRel);
        assert!(
            prev < usize::MAX,
            "opened file description refcount overflow"
        );
    }

    fn release_description_ref(&self) {
        let prev = self.description_refs.fetch_sub(1, Ordering::AcqRel);
        assert!(prev > 0, "opened file description refcount underflow");

        if prev == 1 {
            if let Some(final_release) = self.description_ops.final_release {
                final_release(OpenedFileFinalReleaseCtx {
                    file: self.file.as_ref(),
                    access: self.access,
                    notification_suppressed: self.description_ops.notification_suppressed,
                });
            }
        }
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

// re-export FileOps here, with permission checked.
//
// TODO: we only checked permission of fd, but we haven't checked permission of
// the file itself.
impl FileDesc {
    fn new_unpublished(pfile: Arc<ProcFile>, fd_flags: FdFlags) -> Self {
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

    fn publish_to_fd_table(&self) {
        let already_published = self.published.swap(true, Ordering::AcqRel);
        assert!(
            !already_published,
            "file description published into multiple fd table slots"
        );
        self.pfile.acquire_description_ref();
    }

    fn unpublish_from_fd_table(&self) -> Arc<ProcFile> {
        let was_published = self.published.swap(false, Ordering::AcqRel);
        assert!(was_published, "unpublishing unpublished file descriptor");
        self.pfile.clone()
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

    pub fn set_file_flags(&self, flags: FileStatusFlags) {
        *self.pfile.status_flags.lock() = flags;
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

    fn open_fd(
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

    fn close_fd(&mut self, fd: Fd) -> Result<Arc<ProcFile>, SysError> {
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

    fn get_fd(&self, fd: Fd) -> Result<Arc<FileDesc>, SysError> {
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

    fn dup(&mut self, old_fd: Fd) -> Result<Fd, SysError> {
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
