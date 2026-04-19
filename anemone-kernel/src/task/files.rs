//! File descriptor management for a task.
//!
//! Reference:
//! - https://elixir.bootlin.com/linux/v6.6.32/source/include/linux/fdtable.h

use crate::{net::user_socket::UserSocket, prelude::*};

#[derive(Debug)]
pub struct ProcFile {
    /// Vfs file.
    file: File,
    flags: FileFlags,
}

#[derive(Debug, Clone)]
pub enum FileDesc {
    Vfs {
        pfile: Arc<ProcFile>,
        flags: FdFlags,
    },
    Socket {
        socket: UserSocket,
        flags: FdFlags,
        file_flags: FileFlags,
    },
}

// re-export FileOps here, with permission checked.
//
// TODO: we only checked permission of fd, but we haven't checked permission of
// the file itself.
impl FileDesc {
    fn new_vfs(pfile: Arc<ProcFile>, flags: FdFlags) -> Self {
        Self::Vfs { pfile, flags }
    }

    pub fn new_socket(socket: UserSocket, flags: FdFlags, file_flags: FileFlags) -> Self {
        Self::Socket {
            socket,
            flags,
            file_flags,
        }
    }

    pub fn as_vfs_file(&self) -> Option<&File> {
        match self {
            FileDesc::Vfs { pfile, .. } => Some(&pfile.file),
            FileDesc::Socket { .. } => None,
        }
    }

    pub fn vfs_file(&self) -> &File {
        self.as_vfs_file()
            .expect("socket fd used where a VFS file is required")
    }

    pub fn file_flags(&self) -> FileFlags {
        match self {
            FileDesc::Vfs { pfile, .. } => pfile.flags,
            FileDesc::Socket { file_flags, .. } => *file_flags,
        }
    }

    pub fn fd_flags(&self) -> FdFlags {
        match self {
            FileDesc::Vfs { flags, .. } | FileDesc::Socket { flags, .. } => *flags,
        }
    }

    pub fn user_socket(&self) -> Option<&UserSocket> {
        match self {
            FileDesc::Socket { socket, .. } => Some(socket),
            FileDesc::Vfs { .. } => None,
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, SysError> {
        match self {
            FileDesc::Vfs { pfile, .. } => {
                if !pfile.flags.contains(FileFlags::READ) {
                    return Err(KernelError::PermissionDenied.into());
                }
                pfile.file.read(buf).map_err(|e| e.into())
            }
            FileDesc::Socket {
                socket,
                file_flags,
                ..
            } => {
                if !file_flags.contains(FileFlags::READ) {
                    return Err(KernelError::PermissionDenied.into());
                }
                crate::net::user_socket::user_socket_read(&socket.inner, buf, *file_flags)
            }
        }
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, SysError> {
        match self {
            FileDesc::Vfs { pfile, .. } => {
                if !pfile.flags.contains(FileFlags::WRITE) {
                    return Err(KernelError::PermissionDenied.into());
                }

                if pfile.flags.contains(FileFlags::APPEND) {
                    pfile
                        .file
                        .seek(pfile.file.get_attr()?.size as usize)?;
                }

                pfile.file.write(buf).map_err(|e| e.into())
            }
            FileDesc::Socket {
                socket,
                file_flags,
                ..
            } => {
                if !file_flags.contains(FileFlags::WRITE) {
                    return Err(KernelError::PermissionDenied.into());
                }
                crate::net::user_socket::user_socket_write(&socket.inner, buf, *file_flags)
            }
        }
    }

    /// `whence` is Linux-specific. we handle that in syscall handler. it should
    /// not pollute our FileDesc API.
    pub fn seek(&self, offset: usize) -> Result<(), SysError> {
        match self {
            FileDesc::Vfs { pfile, .. } => pfile.file.seek(offset).map_err(|e| e.into()),
            FileDesc::Socket { .. } => Err(KernelError::Errno(anemone_abi::errno::ESPIPE).into()),
        }
    }

    pub fn iterate(&self, ctx: &mut DirContext) -> Result<DirEntry, SysError> {
        match self {
            FileDesc::Vfs { pfile, .. } => pfile.file.iterate(ctx).map_err(|e| e.into()),
            FileDesc::Socket { .. } => Err(KernelError::Errno(anemone_abi::errno::ENOTDIR).into()),
        }
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FileFlags: u32 {
        const READ = 0b0001;
        const WRITE = 0b0010;
        const APPEND = 0b0100;
        /// `O_NONBLOCK` / `SOCK_NONBLOCK` (non-blocking socket I/O).
        const NONBLOCK = 0b1000;

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
        if flags & anemone_abi::fs::linux::open::O_NONBLOCK != 0 {
            open_flags |= Self::NONBLOCK;
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

    /// dup3 only allows O_CLOEXEC
    pub fn from_dup3_flags(flags: u32) -> Result<Self, SysError> {
        let allowed = anemone_abi::fs::linux::open::O_CLOEXEC;
        if flags & !allowed != 0 {
            return Err(KernelError::InvalidArgument.into());
        }

        Ok(Self::from_linux_open_flags(flags))
    }
}

#[derive(Debug)]
pub struct FilesState {
    // TODO: max_fd
    next_fd: usize,
    recycled_fds: BTreeSet<usize>,
    fd_table: HashMap<usize, Arc<FileDesc>>,
}

impl FilesState {
    fn alloc_fd(&mut self) -> usize {
        if let Some(recycled_fd) = self.recycled_fds.iter().next().cloned() {
            self.recycled_fds.remove(&recycled_fd);
            recycled_fd
        } else {
            while self.fd_table.contains_key(&self.next_fd) {
                self.next_fd += 1;
            }
            let fd = self.next_fd;
            self.next_fd += 1;
            fd
        }
    }

    pub fn new() -> Self {
        Self {
            next_fd: 0,
            recycled_fds: BTreeSet::new(),
            fd_table: HashMap::new(),
        }
    }

    pub fn open_fd(&mut self, file: File, file_flags: FileFlags, fd_flags: FdFlags) -> usize {
        let fd = self.alloc_fd();
        let file = Arc::new(ProcFile {
            file,
            flags: file_flags,
        });

        self.fd_table
            .insert(fd, Arc::new(FileDesc::new_vfs(file, fd_flags)));
        fd
    }

    pub fn open_socket_fd(
        &mut self,
        socket: UserSocket,
        file_flags: FileFlags,
        fd_flags: FdFlags,
    ) -> usize {
        let fd = self.alloc_fd();
        self.fd_table.insert(
            fd,
            Arc::new(FileDesc::new_socket(socket, fd_flags, file_flags)),
        );
        fd
    }

    pub fn get_fd(&self, fd: usize) -> Option<Arc<FileDesc>> {
        self.fd_table.get(&fd).cloned()
    }

    pub fn close_fd(&mut self, fd: usize) -> Option<Arc<FileDesc>> {
        if let Some(file_desc) = self.fd_table.remove(&fd) {
            self.recycled_fds.insert(fd);
            Some(file_desc)
        } else {
            None
        }
    }

    pub fn dup(&mut self, old_fd: usize) -> Option<usize> {
        let fd = self.get_fd(old_fd)?;
        let new_fd = self.alloc_fd();
        let new_desc = match &*fd {
            FileDesc::Vfs { pfile, .. } => FileDesc::new_vfs(pfile.clone(), FdFlags::empty()),
            FileDesc::Socket {
                socket,
                file_flags,
                ..
            } => FileDesc::new_socket(socket.clone(), FdFlags::empty(), *file_flags),
        };
        self.fd_table.insert(new_fd, Arc::new(new_desc));
        Some(new_fd)
    }

    /// Linux's semantics of dup3 is a bit weird, currently we implement a
    /// reasonable subset of it. If in the future we get stuck with
    /// compatibility issues, we'll implement the rest of it.
    pub fn dup3(&mut self, old_fd: usize, new_fd: usize, flags: FdFlags) -> Result<(), SysError> {
        if old_fd == new_fd {
            return Err(KernelError::InvalidArgument.into());
        }

        let fd = self.get_fd(old_fd).ok_or(KernelError::BadFileDescriptor)?;

        if self.fd_table.contains_key(&new_fd) {
            self.close_fd(new_fd);
        }

        // we need to remove new_fd from recycled_fds, because after dup3, new_fd is no
        // longer available for allocation, though new_fd might not be in recycled_fds
        // if new_fd is larger than any previously allocated fd.
        self.recycled_fds.remove(&new_fd);

        if new_fd >= self.next_fd {
            self.next_fd = new_fd + 1;
        }

        let new_desc = match &*fd {
            FileDesc::Vfs { pfile, .. } => FileDesc::new_vfs(pfile.clone(), flags),
            FileDesc::Socket {
                socket,
                file_flags,
                ..
            } => FileDesc::new_socket(socket.clone(), flags, *file_flags),
        };
        self.fd_table.insert(new_fd, Arc::new(new_desc));

        Ok(())
    }

    /// TODO: explain why this is unsafe.
    ///
    /// It's actually quite obvious.
    unsafe fn open_fd_at(
        &mut self,
        fd: usize,
        file: File,
        file_flags: FileFlags,
        fd_flags: FdFlags,
    ) {
        if self.fd_table.contains_key(&fd) {
            self.close_fd(fd);
        }
        self.recycled_fds.remove(&fd);
        if fd >= self.next_fd {
            self.next_fd = fd + 1;
        }
        self.fd_table.insert(
            fd,
            Arc::new(FileDesc::new_vfs(
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
            .map(|(fd, file_desc)| (*fd, Arc::new(file_desc.as_ref().clone())))
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
    pub fn open_fd(&self, file: File, file_flags: FileFlags, fd_flags: FdFlags) -> usize {
        let mut files_state = self.files_state.write();
        files_state.open_fd(file, file_flags, fd_flags)
    }

    pub fn open_socket_fd(&self, socket: UserSocket, file_flags: FileFlags, fd_flags: FdFlags) -> usize {
        let mut files_state = self.files_state.write();
        files_state.open_socket_fd(socket, file_flags, fd_flags)
    }

    pub fn get_fd(&self, fd: usize) -> Option<Arc<FileDesc>> {
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

    pub fn close_fd(&self, fd: usize) -> Option<Arc<FileDesc>> {
        let mut files_state = self.files_state.write();
        files_state.close_fd(fd)
    }

    pub fn dup(&self, old_fd: usize) -> Option<usize> {
        let mut files_state = self.files_state.write();
        files_state.dup(old_fd)
    }

    pub fn dup3(&self, old_fd: usize, new_fd: usize, flags: FdFlags) -> Result<usize, SysError> {
        let mut files_state = self.files_state.write();
        files_state.dup3(old_fd, new_fd, flags)?;
        Ok(new_fd)
    }

    pub fn close_cloexec_fds(&self) {
        self.files_state.write().close_on_exec();
    }
}
