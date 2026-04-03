//! File descriptor management for a task.
//!
//! Reference:
//! - https://elixir.bootlin.com/linux/v6.6.32/source/include/linux/fdtable.h

use crate::prelude::*;

#[derive(Debug)]
pub struct FileDesc {
    fd: usize,
    file: File,
    flags: OpenFlags,
}

// re-export FileOps here, with permission checked.
//
// TODO: we only checked permission of fd, but we haven't checked permission of
// the file itself.
impl FileDesc {
    pub fn fd(&self) -> usize {
        self.fd
    }

    pub fn vfs_file(&self) -> &File {
        &self.file
    }

    pub fn open_flags(&self) -> OpenFlags {
        self.flags
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, SysError> {
        if !self.flags.contains(OpenFlags::READ) {
            return Err(KernelError::PermissionDenied.into());
        }
        self.file.read(buf).map_err(|e| e.into())
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, SysError> {
        if !self.flags.contains(OpenFlags::WRITE) {
            return Err(KernelError::PermissionDenied.into());
        }

        // currently we don't support atomic append, so we just seek to the end of file
        // before writing.

        if self.flags.contains(OpenFlags::APPEND) {
            self.file.seek(self.file.get_attr()?.size as usize)?;
        }

        self.file.write(buf).map_err(|e| e.into())
    }

    /// `whence` is Linux-specific. we handle that in syscall handler. it should
    /// not pollute our FileDesc API.
    pub fn seek(&self, offset: usize) -> Result<(), SysError> {
        self.file.seek(offset).map_err(|e| e.into())
    }

    pub fn iterate(&self, ctx: &mut DirContext) -> Result<DirEntry, SysError> {
        self.file.iterate(ctx).map_err(|e| e.into())
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct OpenFlags: u32 {
        const READ = 0b0001;
        const WRITE = 0b0010;
        const APPEND = 0b0100;

        // create, truncate are not persistant flags, they are only used when opening a file, so we don't need to store them in FileDesc.
    }
}

impl OpenFlags {
    pub fn from_linux_flags(flags: u32) -> Self {
        let mut open_flags = OpenFlags::empty();
        // 1. rw bits
        match flags & 0b11 {
            anemone_abi::fs::linux::open::O_RDONLY => {
                open_flags |= OpenFlags::READ;
            },
            anemone_abi::fs::linux::open::O_WRONLY => {
                open_flags |= OpenFlags::WRITE;
            },
            anemone_abi::fs::linux::open::O_RDWR => {
                open_flags |= OpenFlags::READ | OpenFlags::WRITE;
            },
            _ => {},
        }
        // 2. append bit
        if flags & anemone_abi::fs::linux::open::O_APPEND != 0 {
            open_flags |= OpenFlags::APPEND;
        }

        open_flags
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
    pub fn new() -> Self {
        Self {
            next_fd: 0,
            recycled_fds: BTreeSet::new(),
            fd_table: HashMap::new(),
        }
    }

    pub fn new_with_stdio() -> Self {
        let mut state = Self::new();
        state.open_fd(device::console::open_console_stdin(), OpenFlags::READ);
        state.open_fd(device::console::open_console_stdout(), OpenFlags::WRITE);
        state.open_fd(device::console::open_console_stdout(), OpenFlags::WRITE);
        state
    }

    pub fn open_fd(&mut self, file: File, flags: OpenFlags) -> usize {
        while self.fd_table.contains_key(&self.next_fd) {
            self.next_fd += 1;
        }
        let fd = self.next_fd;
        self.next_fd += 1;

        self.fd_table
            .insert(fd, Arc::new(FileDesc { fd, file, flags }));
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
        let new_fd = if let Some(recycled_fd) = self.recycled_fds.iter().next().cloned() {
            self.recycled_fds.remove(&recycled_fd);
            recycled_fd
        } else {
            let fd = self.next_fd;
            self.next_fd += 1;
            fd
        };
        self.fd_table.insert(new_fd, fd);
        Some(new_fd)
    }

    /// Linux's semantics of dup3 is a bit weird, currently we implement a
    /// reasonable subset of it. If in the future we get stuck with
    /// compatibility issues, we'll implement the rest of it.
    pub fn dup3(&mut self, old_fd: usize, new_fd: usize, flags: OpenFlags) -> Result<(), SysError> {
        if old_fd == new_fd {
            return Err(KernelError::InvalidArgument.into());
        }

        let fd = self.get_fd(old_fd).ok_or(KernelError::BadFileDescriptor)?;

        if self.fd_table.contains_key(&new_fd) {
            self.close_fd(new_fd);
        }

        self.fd_table.insert(new_fd, fd);

        Ok(())
    }

    /// TODO: explain why this is unsafe.
    ///
    /// It's actually quite obvious.
    unsafe fn open_fd_at(&mut self, fd: usize, file: File, flags: OpenFlags) {
        if self.fd_table.contains_key(&fd) {
            self.close_fd(fd);
        }
        self.fd_table
            .insert(fd, Arc::new(FileDesc { fd, file, flags }));
    }
}

impl Task {
    pub fn ensure_stdio(&self, stdin: File, stdout: File, stderr: File) {
        let mut files_state = self.files_state.write();
        unsafe {
            files_state.open_fd_at(0, stdin, OpenFlags::READ);
            files_state.open_fd_at(1, stdout, OpenFlags::WRITE);
            files_state.open_fd_at(2, stderr, OpenFlags::WRITE);
        }
    }

    pub fn open_fd(&self, file: File, flags: OpenFlags) -> usize {
        let mut files_state = self.files_state.write();
        files_state.open_fd(file, flags)
    }

    pub fn get_fd(&self, fd: usize) -> Option<Arc<FileDesc>> {
        let files_state = self.files_state.read();
        files_state.get_fd(fd)
    }

    pub fn close_fd(&self, fd: usize) -> Option<Arc<FileDesc>> {
        let mut files_state = self.files_state.write();
        files_state.close_fd(fd)
    }

    pub fn dup(&self, old_fd: usize) -> Option<usize> {
        let mut files_state = self.files_state.write();
        files_state.dup(old_fd)
    }

    pub fn dup3(&self, old_fd: usize, new_fd: usize, flags: OpenFlags) -> Result<usize, SysError> {
        let mut files_state = self.files_state.write();
        files_state.dup3(old_fd, new_fd, flags)?;
        Ok(new_fd)
    }
}
