//! Virtual file system and filesystem drivers.

// vfs infrastructure
mod anonymous;
mod cache_stats;
mod dentry;
mod epoll;
mod eventfd;
pub mod fanotify;
mod flock;
// mod error;
mod file;
mod filesystem;
mod inode;
mod inode_shrinker;
mod iomux;
mod mount;
mod namei;
mod path;
mod permission;
mod superblock;
mod timerfd;
mod uio;

// filesystem drivers
pub mod devfs;
#[cfg(feature = "fs_ext4")]
mod ext4;
mod pipe;

pub mod proc;

mod ramfs;
mod socket;

pub mod api;

#[cfg(feature = "kunit")]
pub(crate) use self::iomux::IomuxWaitRound;
pub use self::{
    anonymous::*,
    dentry::Dentry,
    file::{
        BackingFileHandle, DirEntry, DirSink, FcntlAccess, FcntlCtx, File, FileFcntlCmd,
        FileFcntlHook, FileFcntlOutcome, FileIoCtx, FileMode, FileOpStatusFlags, FileOps,
        FixedSizeDirSink, IoctlArgFdLookup, IoctlArgFile, IoctlCtx, IoctlFileAccess, ReadDirResult,
        SeekFrom, SinkResult, accept_file_op_status_flags, seek_dir_rewind, seek_with_bounded_size,
        seek_with_fixed_size, seek_with_inode_size,
    },
    filesystem::{FileSystem, FileSystemFlags, FileSystemOps},
    inode::{
        DeviceId, Ino, InoIsZero, InodeMeta, InodeMode, InodeOps, InodePerm, InodeRef, InodeStat,
        InodeType, MakeNodeDescription, ModifType, OpenedFile,
    },
    iomux::{PollEvent, PollRegisterResult, PollRequest},
    mount::{Mount, MountAttrFlags, MountData, MountSource},
    namei::{
        ResolveFlags, resolve, resolve_from, resolve_from_with_root,
        resolve_from_with_root_checked, resolve_parent, resolve_parent_from,
        resolve_parent_from_with_root, resolve_parent_from_with_root_checked,
    },
    path::PathRef,
    permission::{FsAccess, FsPermChecker},
    superblock::SuperBlock,
};
pub(crate) use self::{
    flock::{FlockMode, FlockOperation, FlockOutcome, request_flock, retire_flock},
    inode::{RenameFlags, reject_make_node},
    iomux::PollRoute,
    uio::{UserBufferSegment, UserBufferSink, UserBufferSource},
};
pub use cache_stats::resident_file_inode_cache_pages;
mod vfs;
pub use vfs::*;

use crate::initcall::{InitCallLevel, run_initcalls};

pub fn register_filesystem_drivers() {
    unsafe {
        run_initcalls(InitCallLevel::Fs);
    }
}
