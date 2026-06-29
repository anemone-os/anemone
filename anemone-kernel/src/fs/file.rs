use crate::{
    fs::{
        iomux::{PollEvent, PollRegisterResult, PollRequest},
        uio::{UserBufferSink, UserBufferSource},
    },
    prelude::*,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// Open-time VFS behavior flags for one file object.
    ///
    /// This is not a Linux UAPI bitset or a mirror of Linux `fmode_t` values.
    pub struct FileMode: u32 {
        /// Ordinary read/write do not consume the VFS-managed cursor.
        const STREAM = 0b0001;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// Normalized internal view of opened-description status flags.
    ///
    /// This is not a Linux UAPI bitset and must not become a second owner of
    /// mutable status state. Callers pass it as a short-lived snapshot for one
    /// operation or one candidate-status validation.
    pub struct FileOpStatusFlags: u32 {
        const APPEND = 0b0001;
        const NONBLOCK = 0b0010;
        const DIRECT = 0b0100;
        const DSYNC = 0b1000;
        const SYNC = 0b1_0000;
        const NOATIME = 0b10_0000;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIoCtx {
    status_flags: FileOpStatusFlags,
}

impl FileIoCtx {
    pub const fn new(status_flags: FileOpStatusFlags) -> Self {
        Self { status_flags }
    }

    pub fn blocking() -> Self {
        Self::new(FileOpStatusFlags::empty())
    }

    pub const fn status_flags(self) -> FileOpStatusFlags {
        self.status_flags
    }
}

/// File-object fcntl commands that may be handled by VFS backends.
///
/// The full Linux command set stays at the syscall adapter so fd-table,
/// fd-local, and opened-description status commands cannot be implemented by
/// backend hooks by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFcntlCmd {
    GetPipeSize,
    SetPipeSize,
}

/// Short-lived snapshot for one backend fcntl operation.
///
/// This carries only file-object facts. `O_PATH` and fd-table policy are
/// rejected before this context is built, and mutable status flags remain owned
/// by the opened file description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FcntlAccess {
    can_read: bool,
    can_write: bool,
    status_flags: FileOpStatusFlags,
}

impl FcntlAccess {
    pub const fn new(can_read: bool, can_write: bool, status_flags: FileOpStatusFlags) -> Self {
        Self {
            can_read,
            can_write,
            status_flags,
        }
    }

    pub const fn can_read(self) -> bool {
        self.can_read
    }

    pub const fn can_write(self) -> bool {
        self.can_write
    }

    pub const fn status_flags(self) -> FileOpStatusFlags {
        self.status_flags
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FcntlCtx {
    cmd: FileFcntlCmd,
    arg: u64,
    access: FcntlAccess,
}

impl FcntlCtx {
    pub const fn new(cmd: FileFcntlCmd, arg: u64, access: FcntlAccess) -> Self {
        Self { cmd, arg, access }
    }

    pub const fn cmd(&self) -> FileFcntlCmd {
        self.cmd
    }

    pub const fn arg(&self) -> u64 {
        self.arg
    }

    pub const fn access(&self) -> FcntlAccess {
        self.access
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoctlFileAccess {
    can_read: bool,
    can_write: bool,
    path_only: bool,
    status_flags: FileOpStatusFlags,
}

impl IoctlFileAccess {
    pub const fn new(
        can_read: bool,
        can_write: bool,
        path_only: bool,
        status_flags: FileOpStatusFlags,
    ) -> Self {
        Self {
            can_read,
            can_write,
            path_only,
            status_flags,
        }
    }

    pub const fn can_read(self) -> bool {
        self.can_read
    }

    pub const fn can_write(self) -> bool {
        self.can_write
    }

    pub const fn is_path_only(self) -> bool {
        self.path_only
    }

    pub const fn status_flags(self) -> FileOpStatusFlags {
        self.status_flags
    }
}

#[derive(Debug, Clone)]
pub struct IoctlArgFile {
    file: Arc<File>,
    access: IoctlFileAccess,
}

impl IoctlArgFile {
    pub fn new(file: Arc<File>, access: IoctlFileAccess) -> Self {
        Self { file, access }
    }

    pub fn file(&self) -> &File {
        self.file.as_ref()
    }

    pub fn file_handle(&self) -> Arc<File> {
        self.file.clone()
    }

    pub const fn access(&self) -> IoctlFileAccess {
        self.access
    }
}

pub type IoctlArgFdLookupFn = fn(raw_fd: u64) -> Result<IoctlArgFile, SysError>;

pub struct IoctlArgFdLookup {
    lookup: IoctlArgFdLookupFn,
}

impl IoctlArgFdLookup {
    pub const fn new(lookup: IoctlArgFdLookupFn) -> Self {
        Self { lookup }
    }

    pub fn lookup(&self, raw_fd: u64) -> Result<IoctlArgFile, SysError> {
        (self.lookup)(raw_fd)
    }
}

pub struct IoctlCtx<'a> {
    cmd: u32,
    arg: u64,
    target_access: IoctlFileAccess,
    uspace: Arc<UserSpaceHandle>,
    arg_fd_lookup: &'a IoctlArgFdLookup,
}

impl<'a> IoctlCtx<'a> {
    pub fn new(
        cmd: u32,
        arg: u64,
        target_access: IoctlFileAccess,
        uspace: Arc<UserSpaceHandle>,
        arg_fd_lookup: &'a IoctlArgFdLookup,
    ) -> Self {
        Self {
            cmd,
            arg,
            target_access,
            uspace,
            arg_fd_lookup,
        }
    }

    pub const fn cmd(&self) -> u32 {
        self.cmd
    }

    pub const fn arg(&self) -> u64 {
        self.arg
    }

    pub const fn target_access(&self) -> IoctlFileAccess {
        self.target_access
    }

    pub fn uspace(&self) -> &Arc<UserSpaceHandle> {
        &self.uspace
    }

    pub fn lookup_arg_fd(&self) -> Result<IoctlArgFile, SysError> {
        self.arg_fd_lookup.lookup(self.arg)
    }

    pub fn lookup_fd_arg(&self, raw_fd: u64) -> Result<IoctlArgFile, SysError> {
        self.arg_fd_lookup.lookup(raw_fd)
    }
}

#[derive(Debug, Clone)]
pub struct BackingFileHandle {
    file: Arc<File>,
    writable: bool,
    display_name: String,
}

impl BackingFileHandle {
    pub fn from_ioctl_arg_file(backing: IoctlArgFile) -> Result<Self, SysError> {
        let access = backing.access();
        if access.is_path_only() || !access.can_read() {
            return Err(SysError::BadFileDescriptor);
        }

        let file = backing.file_handle();
        if file.inode().ty() != InodeType::Regular {
            return Err(SysError::InvalidArgument);
        }

        Ok(Self {
            display_name: format!("{}", file.path()),
            file,
            writable: access.can_write(),
        })
    }

    pub const fn can_write(&self) -> bool {
        self.writable
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn get_attr(&self) -> Result<InodeStat, SysError> {
        self.file.get_attr()
    }

    pub fn visible_size(&self) -> Result<usize, SysError> {
        usize::try_from(self.get_attr()?.size).map_err(|_| SysError::FileTooLarge)
    }

    pub fn read_exact_at(&self, mut offset: usize, mut buf: &mut [u8]) -> Result<(), SysError> {
        while !buf.is_empty() {
            let read = self.file.read_at(offset, buf)?;
            if read == 0 {
                return Err(SysError::UnexpectedEof);
            }

            offset = offset.checked_add(read).ok_or(SysError::InvalidArgument)?;
            buf = &mut buf[read..];
        }

        Ok(())
    }

    pub fn write_all_at(&self, mut offset: usize, mut buf: &[u8]) -> Result<(), SysError> {
        if !self.writable {
            return Err(SysError::BadFileDescriptor);
        }

        while !buf.is_empty() {
            let written = self.file.write_at(offset, buf)?;
            if written == 0 {
                return Err(SysError::IO);
            }

            offset = offset
                .checked_add(written)
                .ok_or(SysError::InvalidArgument)?;
            buf = &buf[written..];
        }

        Ok(())
    }
}

/// VTable a file must implement to support file operations.
#[derive(Debug)]
pub struct FileOps {
    pub read: fn(&File, pos: &mut usize, buf: &mut [u8], ctx: FileIoCtx) -> Result<usize, SysError>,
    pub write: fn(&File, pos: &mut usize, buf: &[u8], ctx: FileIoCtx) -> Result<usize, SysError>,
    pub read_at: fn(&File, pos: usize, buf: &mut [u8], ctx: FileIoCtx) -> Result<usize, SysError>,
    pub write_at: fn(&File, pos: usize, buf: &[u8], ctx: FileIoCtx) -> Result<usize, SysError>,
    pub(crate) read_user_at: Option<ReadUserAtHook>,
    pub(crate) write_user_at: Option<WriteUserAtHook>,
    /// Validate a candidate opened-description status snapshot.
    ///
    /// This hook must be side-effect free: it may reject unsupported status
    /// combinations, but it must not cache flags or change later I/O behavior.
    pub check_status_flags: fn(&File, FileOpStatusFlags) -> Result<(), SysError>,
    pub seek: fn(&File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError>,

    /// Read a batch of directory entries starting at `pos` into `sink`.
    ///
    /// Return `ReadDirResult::Progressed` when this call successfully hands at
    /// least one new entry to the sink. Return `ReadDirResult::Eof` only when
    /// the directory is already exhausted before any new entry is accepted.
    pub read_dir:
        fn(&File, pos: &mut usize, sink: &mut dyn DirSink) -> Result<ReadDirResult, SysError>,

    /// Check if the file is ready for IO operations described by `request`.
    ///
    /// Snapshot requests return `Ready(events)`, including an empty ready set.
    /// Register requests must return `Armed` only after the source has saved
    /// the request's latch trigger under the same lock that checked readiness.
    /// Sources that cannot arm a not-ready register request must return
    /// `Unsupported`, so syscall code cannot sleep on an unarmed source.
    pub poll: for<'a> fn(&File, &PollRequest<'a>) -> Result<PollRegisterResult, SysError>,

    /// Optional backend hook for the narrowed file-object fcntl subset.
    ///
    /// Generic fd-owned commands must stay outside this hook. Returning
    /// `Unhandled` asks the VFS wrapper to apply the command-family default
    /// errno; returning `Err` means the backend handled the command and chose
    /// that failure.
    pub fcntl: Option<FileFcntlHook>,
    pub ioctl: for<'a> fn(&File, IoctlCtx<'a>) -> Result<u64, SysError>,
}

pub(crate) type ReadUserAtHook = for<'a> fn(
    &File,
    pos: usize,
    dst: &mut UserBufferSink<'a>,
    ctx: FileIoCtx,
) -> Result<(), SysError>;

pub(crate) type WriteUserAtHook = for<'a> fn(
    &File,
    pos: usize,
    src: &mut UserBufferSource<'a>,
    ctx: FileIoCtx,
) -> Result<usize, SysError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFcntlOutcome {
    Handled(u64),
    Unhandled,
}

pub type FileFcntlHook = fn(&File, &FcntlCtx) -> Result<FileFcntlOutcome, SysError>;

pub fn accept_file_op_status_flags(
    _file: &File,
    _flags: FileOpStatusFlags,
) -> Result<(), SysError> {
    Ok(())
}

fn default_file_fcntl(_file: &File, ctx: &FcntlCtx) -> Result<u64, SysError> {
    match ctx.cmd() {
        FileFcntlCmd::GetPipeSize | FileFcntlCmd::SetPipeSize => Err(SysError::BadFileDescriptor),
    }
}

mod seek {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SeekFrom {
        Set(i64),
        Cur(i64),
        End(i64),
    }

    fn apply_seek_offset(base: usize, offset: i64) -> Result<usize, SysError> {
        if offset >= 0 {
            let offset = usize::try_from(offset).map_err(|_| SysError::FileTooLarge)?;
            base.checked_add(offset).ok_or(SysError::FileTooLarge)
        } else {
            let offset =
                usize::try_from(offset.unsigned_abs()).map_err(|_| SysError::InvalidArgument)?;
            base.checked_sub(offset).ok_or(SysError::InvalidArgument)
        }
    }

    pub fn seek_with_fixed_size(
        _file: &File,
        pos: &mut usize,
        from: SeekFrom,
        size: usize,
    ) -> Result<usize, SysError> {
        let base = match from {
            SeekFrom::Set(_) => 0,
            SeekFrom::Cur(_) => *pos,
            SeekFrom::End(_) => size,
        };
        let offset = match from {
            SeekFrom::Set(offset) | SeekFrom::Cur(offset) | SeekFrom::End(offset) => offset,
        };

        let new_pos = apply_seek_offset(base, offset)?;
        *pos = new_pos;
        Ok(new_pos)
    }

    pub fn seek_with_inode_size(
        file: &File,
        pos: &mut usize,
        from: SeekFrom,
    ) -> Result<usize, SysError> {
        let size = usize::try_from(file.inode().size()).map_err(|_| SysError::FileTooLarge)?;
        seek_with_fixed_size(file, pos, from, size)
    }

    pub fn seek_with_bounded_size(
        file: &File,
        pos: &mut usize,
        from: SeekFrom,
        size: usize,
    ) -> Result<usize, SysError> {
        let mut candidate = *pos;
        let new_pos = seek_with_fixed_size(file, &mut candidate, from, size)?;
        if new_pos > size {
            return Err(SysError::InvalidArgument);
        }
        *pos = new_pos;
        Ok(new_pos)
    }

    pub fn seek_dir_rewind(
        _file: &File,
        pos: &mut usize,
        from: SeekFrom,
    ) -> Result<usize, SysError> {
        match from {
            SeekFrom::Set(0) => {
                *pos = 0;
                Ok(0)
            },
            _ => Err(SysError::InvalidArgument),
        }
    }

    pub(super) fn seek_set_offset(pos: usize) -> Result<i64, SysError> {
        i64::try_from(pos).map_err(|_| SysError::FileTooLarge)
    }
}

use self::seek::seek_set_offset;
pub use self::seek::{
    SeekFrom, seek_dir_rewind, seek_with_bounded_size, seek_with_fixed_size, seek_with_inode_size,
};

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub ino: Ino,
    pub ty: InodeType,
}

#[derive(Debug, Clone, Copy)]
pub enum ReadDirResult {
    /// At least one new directory entry was accepted by the sink.
    Progressed,
    /// The directory was already exhausted before any new entry was accepted.
    Eof,
}

#[derive(Debug, Clone, Copy)]
pub enum SinkResult {
    /// The sink accepted this entry, so the producer may advance its cursor.
    Accepted,
    /// The sink wants to stop before consuming this entry.
    ///
    /// Producers must not advance the directory cursor for the current entry.
    /// Sinks that cannot accept even the first entry of a batch should return
    /// [ReadDirResult::Eof] instead of [SinkResult::Stop].
    Stop,
}

/// Trait instead of concrete struct thus allowing more flexible
/// implementations. e.g. fixed-capacity array, zero-copy buffer, etc.
pub trait DirSink {
    fn push(&mut self, entry: DirEntry) -> Result<SinkResult, SysError>;
}

#[derive(Debug, Clone)]
pub struct FixedSizeDirSink<const N: usize> {
    entries: Vec<DirEntry>,
}

impl<const N: usize> FixedSizeDirSink<N> {
    pub fn new() -> Self {
        const_assert!(N > 0, "FixedSizeDirSink must have positive capacity");

        Self {
            entries: Vec::new(),
        }
    }

    pub fn entries(&self) -> &[DirEntry] {
        &self.entries
    }

    pub fn entries_mut(&mut self) -> &mut [DirEntry] {
        &mut self.entries
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl<const N: usize> DirSink for FixedSizeDirSink<N> {
    fn push(&mut self, entry: DirEntry) -> Result<SinkResult, SysError> {
        if self.entries.len() < N {
            self.entries.push(entry);
            Ok(SinkResult::Accepted)
        } else {
            Ok(SinkResult::Stop)
        }
    }
}

#[derive(Debug)]
pub struct File {
    path: PathRef,
    ops: &'static FileOps,
    /// Immutable VFS behavior selected by the open path.
    ///
    /// This is not mutable file status and must not be changed by fcntl.
    mode: FileMode,
    prv: AnyOpaque,
    pos: Mutex<usize>,
}

impl File {
    pub(super) fn new(path: PathRef, ops: &'static FileOps, prv: AnyOpaque) -> Self {
        Self::new_with_mode(path, ops, FileMode::empty(), prv)
    }

    pub(super) fn new_with_mode(
        path: PathRef,
        ops: &'static FileOps,
        mode: FileMode,
        prv: AnyOpaque,
    ) -> Self {
        Self {
            path,
            ops,
            mode,
            prv,
            pos: Mutex::new(0),
        }
    }

    pub(super) fn path_only(path: PathRef) -> Self {
        static PATH_ONLY_FILE_OPS: FileOps = FileOps {
            read: |_, _, _, _| Err(SysError::BadFileDescriptor),
            write: |_, _, _, _| Err(SysError::BadFileDescriptor),
            read_at: |_, _, _, _| Err(SysError::BadFileDescriptor),
            write_at: |_, _, _, _| Err(SysError::BadFileDescriptor),
            read_user_at: None,
            write_user_at: None,
            check_status_flags: accept_file_op_status_flags,
            seek: |_, _, _| Err(SysError::BadFileDescriptor),
            read_dir: |_, _, _| Err(SysError::BadFileDescriptor),
            poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::empty() & req.interests())),
            fcntl: None,
            ioctl: |_, _| Err(SysError::BadFileDescriptor),
        };

        Self::new(path, &PATH_ONLY_FILE_OPS, NilOpaque::new())
    }

    pub(super) fn prv(&self) -> &AnyOpaque {
        &self.prv
    }
}

impl File {
    pub const fn mode(&self) -> FileMode {
        self.mode
    }

    pub fn is_stream(&self) -> bool {
        self.mode().contains(FileMode::STREAM)
    }

    pub fn pos(&self) -> usize {
        *self.pos.lock()
    }
}

// VTable operations re-exported here.
impl File {
    pub fn inode(&self) -> &InodeRef {
        self.path.inode()
    }

    pub fn path(&self) -> &PathRef {
        &self.path
    }

    fn ensure_regular_content_writable(&self) -> Result<(), SysError> {
        if self.inode().ty() == InodeType::Regular {
            self.path.mount().ensure_writable()?;
        }
        Ok(())
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, SysError> {
        self.read_with_ctx(buf, FileIoCtx::blocking())
    }

    pub fn read_with_ctx(&self, buf: &mut [u8], ctx: FileIoCtx) -> Result<usize, SysError> {
        if buf.len() == 0 {
            return Ok(0);
        }

        if self.is_stream() {
            // Stream files keep their cursor in the backend, if they have one.
            // The local value only preserves the existing FileOps signature.
            let mut pos = 0;
            return (self.ops.read)(self, &mut pos, buf, ctx);
        }

        let mut pos = self.pos.lock();
        (self.ops.read)(self, &mut *pos, buf, ctx)
    }

    pub(crate) fn has_read_user_at(&self) -> bool {
        self.ops.read_user_at.is_some()
    }

    fn call_read_user_at(
        &self,
        read_user_at: ReadUserAtHook,
        pos: usize,
        dst: &mut UserBufferSink<'_>,
        ctx: FileIoCtx,
    ) -> Result<usize, SysError> {
        let mark = dst.mark();
        let remaining = dst.remaining();
        let result = read_user_at(self, pos, dst, ctx);
        let copied = dst.bytes_since(mark);
        assert!(
            copied <= remaining,
            "direct-user read advanced beyond requested user-buffer range"
        );

        match result {
            Ok(()) => Ok(copied),
            Err(err) if copied > 0 => Ok(copied),
            Err(err) => Err(err),
        }
    }

    pub(crate) fn read_user_with_ctx(
        &self,
        dst: &mut UserBufferSink<'_>,
        ctx: FileIoCtx,
    ) -> Result<usize, SysError> {
        if dst.remaining() == 0 {
            return Ok(0);
        }
        assert!(
            !self.is_stream(),
            "stream file installed positioned direct-user read hook"
        );
        let read_user_at = self
            .ops
            .read_user_at
            .expect("direct-user read wrapper called without hook");

        let mut pos = self.pos.lock();
        let read = self.call_read_user_at(read_user_at, *pos, dst, ctx)?;
        *pos = pos.checked_add(read).ok_or(SysError::InvalidArgument)?;
        Ok(read)
    }

    /// Reading at specified offset without changing the file cursor.
    pub fn read_at(&self, pos: usize, buf: &mut [u8]) -> Result<usize, SysError> {
        self.read_at_with_ctx(pos, buf, FileIoCtx::blocking())
    }

    pub fn read_at_with_ctx(
        &self,
        pos: usize,
        buf: &mut [u8],
        ctx: FileIoCtx,
    ) -> Result<usize, SysError> {
        if buf.len() == 0 {
            return Ok(0);
        }

        (self.ops.read_at)(self, pos, buf, ctx)
    }

    pub(crate) fn read_user_at_with_ctx(
        &self,
        pos: usize,
        dst: &mut UserBufferSink<'_>,
        ctx: FileIoCtx,
    ) -> Result<usize, SysError> {
        if dst.remaining() == 0 {
            return Ok(0);
        }
        let read_user_at = self
            .ops
            .read_user_at
            .expect("direct-user read wrapper called without hook");
        self.call_read_user_at(read_user_at, pos, dst, ctx)
    }

    pub fn read_exact(&self, mut buf: &mut [u8]) -> Result<(), SysError> {
        if buf.len() == 0 {
            return Ok(());
        }

        let ctx = FileIoCtx::blocking();
        if self.is_stream() {
            let mut pos = 0;
            while !buf.is_empty() {
                let read = (self.ops.read)(self, &mut pos, buf, ctx)?;
                if read == 0 {
                    return Err(SysError::UnexpectedEof);
                }
                buf = &mut buf[read..];
            }

            return Ok(());
        }

        let mut pos = self.pos.lock();
        while !buf.is_empty() {
            let read = (self.ops.read)(self, &mut *pos, buf, ctx)?;
            if read == 0 {
                return Err(SysError::UnexpectedEof);
            }
            buf = &mut buf[read..];
        }

        Ok(())
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, SysError> {
        self.write_with_ctx(buf, FileIoCtx::blocking())
    }

    pub fn write_with_ctx(&self, buf: &[u8], ctx: FileIoCtx) -> Result<usize, SysError> {
        if buf.len() == 0 {
            return Ok(0);
        }

        self.ensure_regular_content_writable()?;

        let cred = get_current_task().cred();
        let written = {
            if self.is_stream() {
                let mut pos = 0;
                (self.ops.write)(self, &mut pos, buf, ctx)?
            } else {
                let mut pos = self.pos.lock();
                (self.ops.write)(self, &mut *pos, buf, ctx)?
            }
        };
        if written > 0 {
            self.inode()
                .after_modified(&cred, ModifType::Modify, Instant::now().to_duration());
        }

        Ok(written)
    }

    /// Writing at specified offset without changing the file cursor.
    pub fn write_at(&self, pos: usize, buf: &[u8]) -> Result<usize, SysError> {
        self.write_at_with_ctx(pos, buf, FileIoCtx::blocking())
    }

    pub fn write_at_with_ctx(
        &self,
        pos: usize,
        buf: &[u8],
        ctx: FileIoCtx,
    ) -> Result<usize, SysError> {
        if buf.len() == 0 {
            return Ok(0);
        }

        self.ensure_regular_content_writable()?;

        let cred = get_current_task().cred();
        let written = (self.ops.write_at)(self, pos, buf, ctx)?;
        if written > 0 {
            self.inode()
                .after_modified(&cred, ModifType::Modify, Instant::now().to_duration());
        }

        Ok(written)
    }

    #[allow(dead_code)]
    pub(crate) fn write_user_at_with_ctx(
        &self,
        pos: usize,
        src: &mut UserBufferSource<'_>,
        ctx: FileIoCtx,
    ) -> Result<usize, SysError> {
        if src.remaining() == 0 {
            return Ok(0);
        }
        self.ensure_regular_content_writable()?;

        let write_user_at = self
            .ops
            .write_user_at
            .expect("direct-user write wrapper called without hook");
        let mark = src.mark();
        let written = write_user_at(self, pos, src, ctx)?;
        assert!(
            written <= src.bytes_since(mark),
            "direct-user write committed more bytes than it copied"
        );
        src.keep_prefix_from(mark, written);
        if written > 0 {
            let cred = get_current_task().cred();
            self.inode()
                .after_modified(&cred, ModifType::Modify, Instant::now().to_duration());
        }
        Ok(written)
    }

    /// Different from [Self::seek] + [Self::write], this is an atomic
    /// operation.
    pub fn append(&self, buf: &[u8]) -> Result<usize, SysError> {
        self.append_with_ctx(buf, FileIoCtx::blocking())
    }

    pub fn append_with_ctx(&self, buf: &[u8], ctx: FileIoCtx) -> Result<usize, SysError> {
        if buf.len() == 0 {
            return Ok(0);
        }

        self.ensure_regular_content_writable()?;

        let sz = self.inode().get_attr()?.size as usize;
        let cred = get_current_task().cred();
        let written = {
            let mut pos = self.pos.lock();
            *pos = sz;
            (self.ops.write)(self, &mut *pos, buf, ctx)?
        };
        if written > 0 {
            self.inode()
                .after_modified(&cred, ModifType::Modify, Instant::now().to_duration());
        }
        Ok(written)
    }

    /// Append without changing the file cursor.
    pub fn append_at_current_end(&self, buf: &[u8]) -> Result<usize, SysError> {
        self.append_at_current_end_with_ctx(buf, FileIoCtx::blocking())
    }

    pub fn append_at_current_end_with_ctx(
        &self,
        buf: &[u8],
        ctx: FileIoCtx,
    ) -> Result<usize, SysError> {
        if buf.len() == 0 {
            return Ok(0);
        }

        self.ensure_regular_content_writable()?;

        let mut append_pos = self.inode().get_attr()?.size as usize;
        let cred = get_current_task().cred();
        let written = {
            let _pos = self.pos.lock();
            (self.ops.write)(self, &mut append_pos, buf, ctx)?
        };
        if written > 0 {
            self.inode()
                .after_modified(&cred, ModifType::Modify, Instant::now().to_duration());
        }
        Ok(written)
    }

    pub fn seek(&self, from: SeekFrom) -> Result<usize, SysError> {
        let mut pos = self.pos.lock();
        let old_pos = *pos;
        match (self.ops.seek)(self, &mut *pos, from) {
            Ok(new_pos) => {
                *pos = new_pos;
                Ok(new_pos)
            },
            Err(err) => {
                *pos = old_pos;
                Err(err)
            },
        }
    }

    pub fn seek_set_checked(&self, pos: usize) -> Result<usize, SysError> {
        self.seek(SeekFrom::Set(seek_set_offset(pos)?))
    }

    pub fn read_dir(&self, sink: &mut dyn DirSink) -> Result<ReadDirResult, SysError> {
        let mut pos = self.pos.lock();
        (self.ops.read_dir)(self, &mut *pos, sink)
    }

    pub fn poll(&self, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
        (self.ops.poll)(self, request)
    }

    pub fn fcntl(&self, ctx: FcntlCtx) -> Result<u64, SysError> {
        if let Some(fcntl) = self.ops.fcntl {
            match fcntl(self, &ctx)? {
                FileFcntlOutcome::Handled(ret) => return Ok(ret),
                FileFcntlOutcome::Unhandled => {},
            }
        }

        default_file_fcntl(self, &ctx)
    }

    pub fn ioctl(&self, ctx: IoctlCtx<'_>) -> Result<u64, SysError> {
        (self.ops.ioctl)(self, ctx)
    }

    pub fn check_status_flags(&self, flags: FileOpStatusFlags) -> Result<(), SysError> {
        (self.ops.check_status_flags)(self, flags)
    }

    pub fn get_attr(&self) -> Result<InodeStat, SysError> {
        self.inode().get_attr()
    }
}
