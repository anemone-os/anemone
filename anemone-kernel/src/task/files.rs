//! File descriptor management for a task.
//!
//! Reference:
//! - https://elixir.bootlin.com/linux/v6.6.32/source/include/linux/fdtable.h

use crate::prelude::{handler::TryFromSyscallArg, *};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
        if (raw >> 32) != 0 {
            Err(SysError::InvalidArgument)
        } else if (raw as i32) < 0 {
            Err(SysError::InvalidArgument)
        } else {
            if (raw >> 32) as u32 >= i32::MAX as u32 {
                Err(SysError::InvalidArgument)
            } else {
                Ok(Self(raw as u32))
            }
        }
    }
}

#[derive(Debug)]
pub struct ProcFile {
    /// Vfs file.
    file: File,
    flags: FileFlags,
}

#[derive(Debug, Clone)]
pub struct FileDesc {
    pfile: Arc<ProcFile>,
    flags: FdFlags,
}

// re-export FileOps here, with permission checked.
//
// TODO: we only checked permission of fd, but we haven't checked permission of
// the file itself.
impl FileDesc {
    fn new(pfile: Arc<ProcFile>, flags: FdFlags) -> Self {
        Self { pfile, flags }
    }

    pub fn vfs_file(&self) -> &File {
        &self.pfile.file
    }

    pub fn file_flags(&self) -> FileFlags {
        self.pfile.flags
    }

    pub fn fd_flags(&self) -> FdFlags {
        self.flags
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, SysError> {
        if !self.pfile.flags.contains(FileFlags::READ) {
            return Err(SysError::PermissionDenied);
        }
        self.pfile.file.read(buf).map_err(|e| e.into())
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, SysError> {
        if !self.pfile.flags.contains(FileFlags::WRITE) {
            return Err(SysError::PermissionDenied);
        }

        // currently we don't support atomic append, so we just seek to the end of file
        // before writing.

        if self.pfile.flags.contains(FileFlags::APPEND) {
            self.pfile
                .file
                .seek(self.pfile.file.get_attr()?.size as usize)?;
        }

        self.pfile.file.write(buf).map_err(|e| e.into())
    }

    /// `whence` is Linux-specific. we handle that in syscall handler. it should
    /// not pollute our FileDesc API.
    pub fn seek(&self, offset: usize) -> Result<(), SysError> {
        self.pfile.file.seek(offset).map_err(|e| e.into())
    }

    pub fn iterate(&self, ctx: &mut DirContext) -> Result<DirEntry, SysError> {
        self.pfile.file.iterate(ctx).map_err(|e| e.into())
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FileFlags: u32 {
        const READ = 0b0001;
        const WRITE = 0b0010;
        const APPEND = 0b0100;

        // create, truncate are not persistant flags, they are only used when opening a file, so we don't need to store them in FileDesc.
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

impl FileFlags {
    pub fn from_linux_open_flags(flags: u32) -> Self {
        let mut open_flags = Self::empty();
        // 1. rw bits
        match flags & 0b11 {
            anemone_abi::fs::linux::open::O_RDONLY => {
                open_flags |= Self::READ;
            },
            anemone_abi::fs::linux::open::O_WRONLY => {
                open_flags |= Self::WRITE;
            },
            anemone_abi::fs::linux::open::O_RDWR => {
                open_flags |= Self::READ | Self::WRITE;
            },
            _ => {},
        }
        // 2. append bit
        if flags & anemone_abi::fs::linux::open::O_APPEND != 0 {
            open_flags |= Self::APPEND;
        }

        open_flags
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

#[derive(Debug)]
pub struct FilesState {
    // TODO: max_fd
    next_fd: Fd,
    recycled_fds: BTreeSet<Fd>,
    fd_table: HashMap<Fd, Arc<FileDesc>>,
}

impl FilesState {
    fn alloc_fd(&mut self) -> Option<Fd> {
        if let Some(recycled_fd) = self.recycled_fds.iter().next().cloned() {
            self.recycled_fds.remove(&recycled_fd);
            Some(recycled_fd)
        } else {
            while self.fd_table.contains_key(&self.next_fd) {
                let next_fd = Fd::new(self.next_fd.raw() + 1)?;
                self.next_fd = next_fd;
            }
            let fd = self.next_fd;
            self.next_fd = Fd::new(self.next_fd.raw() + 1)?;
            Some(fd)
        }
    }

    pub fn new() -> Self {
        Self {
            next_fd: Fd(0),
            recycled_fds: BTreeSet::new(),
            fd_table: HashMap::new(),
        }
    }

    pub fn open_fd(&mut self, file: File, file_flags: FileFlags, fd_flags: FdFlags) -> Option<Fd> {
        let fd = self.alloc_fd()?;
        let file = Arc::new(ProcFile {
            file,
            flags: file_flags,
        });

        self.fd_table
            .insert(fd, Arc::new(FileDesc::new(file, fd_flags)));
        Some(fd)
    }

    pub fn get_fd(&self, fd: Fd) -> Option<Arc<FileDesc>> {
        self.fd_table.get(&fd).cloned()
    }

    pub fn close_fd(&mut self, fd: Fd) -> Option<Arc<FileDesc>> {
        if let Some(file_desc) = self.fd_table.remove(&fd) {
            self.recycled_fds.insert(fd);
            Some(file_desc)
        } else {
            None
        }
    }

    pub fn dup(&mut self, old_fd: Fd) -> Option<Fd> {
        let fd = self.get_fd(old_fd)?;
        let new_fd = self.alloc_fd()?;
        self.fd_table.insert(
            new_fd,
            Arc::new(FileDesc::new(fd.pfile.clone(), FdFlags::empty())),
        );
        Some(new_fd)
    }

    /// Linux's semantics of dup3 is a bit weird, currently we implement a
    /// reasonable subset of it. If in the future we get stuck with
    /// compatibility issues, we'll implement the rest of it.
    pub fn dup3(&mut self, old_fd: Fd, new_fd: Fd, flags: FdFlags) -> Result<(), SysError> {
        if old_fd == new_fd {
            return Err(SysError::InvalidArgument);
        }

        let fd = self.get_fd(old_fd).ok_or(SysError::BadFileDescriptor)?;

        if self.fd_table.contains_key(&new_fd) {
            self.close_fd(new_fd);
        }

        // we need to remove new_fd from recycled_fds, because after dup3, new_fd is no
        // longer available for allocation, though new_fd might not be in recycled_fds
        // if new_fd is larger than any previously allocated fd.
        let exist = self.recycled_fds.remove(&new_fd);

        if new_fd >= self.next_fd {
            match Fd::new(new_fd.raw() + 1) {
                Some(next_fd) => self.next_fd = next_fd,
                None => {
                    if exist {
                        self.recycled_fds.insert(new_fd);
                    }
                    return Err(SysError::InvalidArgument);
                },
            }
        }

        self.fd_table
            .insert(new_fd, Arc::new(FileDesc::new(fd.pfile.clone(), flags)));

        Ok(())
    }

    /// TODO: explain why this is unsafe.
    ///
    /// It's actually quite obvious.
    unsafe fn open_fd_at(&mut self, fd: Fd, file: File, file_flags: FileFlags, fd_flags: FdFlags) {
        if self.fd_table.contains_key(&fd) {
            self.close_fd(fd);
        }
        let exist = self.recycled_fds.remove(&fd);
        if fd >= self.next_fd {
            match Fd::new(fd.raw() + 1) {
                Some(next_fd) => self.next_fd = next_fd,
                None => {
                    if exist {
                        self.recycled_fds.insert(fd);
                    }
                    panic!("fd is too large");
                },
            }
        }
        self.fd_table.insert(
            fd,
            Arc::new(FileDesc::new(
                Arc::new(ProcFile {
                    file,
                    flags: file_flags,
                }),
                fd_flags,
            )),
        );
    }

    pub fn create_copy(&self) -> Self {
        let mut new = Self::new();
        new.next_fd = self.next_fd;
        new.recycled_fds = self.recycled_fds.clone();
        new.fd_table = self
            .fd_table
            .iter()
            // note that we can't clone fd_table directly, since fd flags is per-fd.
            .map(|(fd, file_desc)| {
                (
                    *fd,
                    Arc::new(
                        // this clones file desc itself, not the arc, so that we can have different
                        // fd flags for the new fd table.
                        file_desc.as_ref().clone(),
                    ),
                )
            })
            .collect();
        new
    }

    /// Call this function to close all file descriptors with O_CLOEXEC flag
    /// when executing a new program.
    pub fn close_on_exec(&mut self) {
        let cloexec_fds = self
            .fd_table
            .iter()
            .filter_map(|(fd, file_desc)| {
                file_desc
                    .fd_flags()
                    .contains(FdFlags::CLOSE_ON_EXEC)
                    .then_some(*fd)
            })
            .collect::<Vec<_>>();

        for fd in cloexec_fds {
            self.close_fd(fd);
        }
    }
}

impl Task {
    pub fn open_fd(&self, file: File, file_flags: FileFlags, fd_flags: FdFlags) -> Option<Fd> {
        let mut files_state = self.files_state.write();
        files_state.open_fd(file, file_flags, fd_flags)
    }

    pub fn get_fd(&self, fd: Fd) -> Option<Arc<FileDesc>> {
        let files_state = self.files_state.read();
        files_state.get_fd(fd)
    }

    pub fn files_state(&self) -> Arc<RwLock<FilesState>> {
        self.files_state.clone()
    }

    /// Replace the contents of the current file-table state object.
    ///
    /// If this task is sharing the same file-table handle with other tasks,
    /// they will observe the updated contents as well.
    ///
    /// Note the semantic difference between this function and
    /// [`Self::replace_files_state_handle`].
    pub fn set_files_state(&self, files_state: FilesState) {
        *self.files_state.write() = files_state;
    }

    /// Replace the shared file-table state handle.
    ///
    /// This should only be used while the task is still uniquely owned, such
    /// as during task construction or clone setup.
    pub(super) fn replace_files_state_handle(&mut self, files_state: Arc<RwLock<FilesState>>) {
        self.files_state = files_state;
    }

    pub fn close_fd(&self, fd: Fd) -> Option<Arc<FileDesc>> {
        let mut files_state = self.files_state.write();
        files_state.close_fd(fd)
    }

    pub fn dup(&self, old_fd: Fd) -> Option<Fd> {
        let mut files_state = self.files_state.write();
        files_state.dup(old_fd)
    }

    pub fn dup3(&self, old_fd: Fd, new_fd: Fd, flags: FdFlags) -> Result<Fd, SysError> {
        let mut files_state = self.files_state.write();
        files_state.dup3(old_fd, new_fd, flags)?;
        Ok(new_fd)
    }

    pub fn close_cloexec_fds(&self) {
        self.files_state.write().close_on_exec();
    }
}