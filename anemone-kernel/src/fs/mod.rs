//! Virtual file system and filesystem drivers.

// vfs infrastructure
mod address_space;
mod anonymous;
mod cache_stats;
mod dentry;
mod epoll;
mod eventfd;
pub mod fanotify;
// mod error;
mod file;
mod filesystem;
mod inode;
mod inode_shrinker;
mod iomux;
mod lock;
mod mount;
mod namei;
mod path;
mod permission;
mod superblock;
mod timerfd;
mod uio;

// filesystem drivers
pub mod devfs;
mod devpts;
#[cfg(feature = "fs_ext4")]
mod ext4;
mod pipe;

pub mod proc;

mod ramfs;
mod socket;
mod sysfs;

pub mod api;

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
    file::{FileOpenAccess, FileOpenRequest, IoctlFdInstaller},
    inode::{
        OpenDescriptionActivation, OpenDescriptionCommit, PreparedOpenDescription, RenameFlags,
        reject_make_node,
    },
    iomux::PollRoute,
    lock::{
        FlockMode, FlockOperation, FlockOutcome, PosixLockMode, PosixLockQueryOutcome,
        PosixLockRange, PosixLockSetOutcome, query_posix_lock, request_flock, retire_flock,
        retire_posix_locks, set_posix_lock, unlock_posix_lock,
    },
    uio::{UserBufferSegment, UserBufferSink, UserBufferSource},
    vfs::vfs_open_description,
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

/// Activate public filesystem namespaces whose providers span fs initcalls.
pub(crate) fn activate_public_filesystems() {
    devpts::activate_public_namespace();
}
