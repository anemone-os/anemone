use anemone_abi::fs::linux::{fanotify as abi, open};

use crate::{
    prelude::*,
    task::files::{FdFlags, FileStatusFlags, OpenAccessMode},
};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FanMask: u64 {
        const ACCESS = abi::FAN_ACCESS;
        const MODIFY = abi::FAN_MODIFY;
        const CLOSE_WRITE = abi::FAN_CLOSE_WRITE;
        const CLOSE_NOWRITE = abi::FAN_CLOSE_NOWRITE;
        const OPEN = abi::FAN_OPEN;
        const EVENT_ON_CHILD = abi::FAN_EVENT_ON_CHILD;
        const ONDIR = abi::FAN_ONDIR;
        const Q_OVERFLOW = abi::FAN_Q_OVERFLOW;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FanInitFlags: u32 {
        const CLOEXEC = abi::FAN_CLOEXEC;
        const NONBLOCK = abi::FAN_NONBLOCK;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanGroupMode {
    Notify,
    Content,
    PreContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FanTargetClass {
    Inode,
    Mount,
    Filesystem,
}

#[derive(Debug, Clone)]
pub enum FanTarget {
    Inode(InodeRef),
    Mount(Arc<Mount>),
    Filesystem(Arc<SuperBlock>),
}

impl FanTarget {
    pub fn from_path(class: FanTargetClass, path: &PathRef) -> Self {
        match class {
            FanTargetClass::Inode => Self::Inode(path.inode().clone()),
            FanTargetClass::Mount => Self::Mount(path.mount().clone()),
            FanTargetClass::Filesystem => Self::Filesystem(path.mount().sb().clone()),
        }
    }

    pub fn key(&self) -> FanTargetKey {
        match self {
            Self::Inode(inode) => FanTargetKey::from_inode(inode),
            Self::Mount(mount) => FanTargetKey::from_mount(mount),
            Self::Filesystem(sb) => FanTargetKey::from_superblock(sb),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FanTargetKey {
    Inode { sb: usize, ino: Ino },
    Mount { mount: usize },
    SuperBlock { sb: usize },
}

impl FanTargetKey {
    pub fn from_inode(inode: &InodeRef) -> Self {
        let sb = inode.sb();
        Self::Inode {
            sb: Arc::as_ptr(&sb) as usize,
            ino: inode.ino(),
        }
    }

    pub fn from_mount(mount: &Arc<Mount>) -> Self {
        Self::Mount {
            mount: Arc::as_ptr(mount) as usize,
        }
    }

    pub fn from_superblock(sb: &Arc<SuperBlock>) -> Self {
        Self::SuperBlock {
            sb: Arc::as_ptr(sb) as usize,
        }
    }

    pub const fn class(self) -> FanTargetClass {
        match self {
            Self::Inode { .. } => FanTargetClass::Inode,
            Self::Mount { .. } => FanTargetClass::Mount,
            Self::SuperBlock { .. } => FanTargetClass::Filesystem,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanEventFdTemplate {
    pub(super) access: OpenAccessMode,
    pub(super) status: FileStatusFlags,
    pub(super) fd: FdFlags,
    pub(super) getfl_visible_flags: u32,
    pub(super) accepted_noop_flags: u32,
}

impl FanGroupMode {
    pub fn from_init_flags(flags: u32) -> Result<Self, SysError> {
        let class = flags & (abi::FAN_CLASS_CONTENT | abi::FAN_CLASS_PRE_CONTENT);
        match class {
            abi::FAN_CLASS_NOTIF => Ok(Self::Notify),
            abi::FAN_CLASS_CONTENT => Ok(Self::Content),
            abi::FAN_CLASS_PRE_CONTENT => Ok(Self::PreContent),
            _ => Err(SysError::InvalidArgument),
        }
    }
}

pub fn parse_event_fd_template(raw: u32) -> Result<FanEventFdTemplate, SysError> {
    const VALID: u32 = open::O_ACCMODE
        | open::O_APPEND
        | open::O_NONBLOCK
        | open::O_DSYNC
        | open::O_CLOEXEC
        | open::O_LARGEFILE
        | open::O_NOATIME
        | open::O_SYNC;

    if raw & !VALID != 0 {
        knoticeln!(
            "fanotify_init: unsupported event_f_flags bits={:#x}",
            raw & !VALID
        );
        return Err(SysError::InvalidArgument);
    }

    let access = match raw & open::O_ACCMODE {
        open::O_RDONLY => OpenAccessMode::Read,
        open::O_WRONLY => OpenAccessMode::Write,
        open::O_RDWR => OpenAccessMode::ReadWrite,
        _ => return Err(SysError::InvalidArgument),
    };

    let mut status = FileStatusFlags::empty();
    status.set(FileStatusFlags::APPEND, raw & open::O_APPEND != 0);
    status.set(FileStatusFlags::NONBLOCK, raw & open::O_NONBLOCK != 0);
    status.set(FileStatusFlags::DSYNC, raw & open::O_DSYNC != 0);
    status.set(FileStatusFlags::SYNC, raw & open::O_SYNC == open::O_SYNC);
    status.set(FileStatusFlags::NOATIME, raw & open::O_NOATIME != 0);

    let fd = if raw & open::O_CLOEXEC != 0 {
        FdFlags::CLOSE_ON_EXEC
    } else {
        FdFlags::empty()
    };

    let mut getfl_visible_flags = 0;
    let mut accepted_noop_flags = 0;
    if raw & open::O_LARGEFILE != 0 {
        getfl_visible_flags |= open::O_LARGEFILE;
        accepted_noop_flags |= open::O_LARGEFILE;
    }

    Ok(FanEventFdTemplate {
        access,
        status,
        fd,
        getfl_visible_flags,
        accepted_noop_flags,
    })
}

pub fn init_fd_flags(flags: FanInitFlags) -> FdFlags {
    if flags.contains(FanInitFlags::CLOEXEC) {
        FdFlags::CLOSE_ON_EXEC
    } else {
        FdFlags::empty()
    }
}

pub fn init_file_status_flags(flags: FanInitFlags) -> FileStatusFlags {
    let mut status = FileStatusFlags::empty();
    status.set(
        FileStatusFlags::NONBLOCK,
        flags.contains(FanInitFlags::NONBLOCK),
    );
    status
}
