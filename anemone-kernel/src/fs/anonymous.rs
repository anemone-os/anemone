//! Sometimes you just need a file that doesn't correspond to any real file on
//! disk, but still has an inode and supports being opened and read/written to.
//! This module provides such "anonymous" files.
//!
//! Note that anonymous files cannot guarantee uniqueness of themselves.
//!
//! Note that you shouldn't do any assumptions about the inode behind an
//! anonymous file, they are just a placeholder.
//!
//! TODO: In future we may just turn this into devfs files? 🤔 but now we just
//! want to get the basic console files working.

use crate::{
    fs::inode::Inode,
    prelude::*,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

static ANONYMOUS_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(FsError::NotSupported),
    create: |_, _, _| Err(FsError::NotSupported),
    link: |_, _, _| Err(FsError::NotSupported),
    unlink: |_, _| Err(FsError::NotSupported),
    mkdir: |_, _, _| Err(FsError::NotSupported),
    rmdir: |_, _| Err(FsError::NotSupported),
    open: |_| Err(FsError::NotSupported),
    get_attr: |_| Err(FsError::NotSupported),
};

const ANONYMOUS_INO: u64 = 39;

pub fn vfs_open_anonymous(
    name: impl AsRef<str>,
    mode: InodeMode,
    file_ops: &'static FileOps,
    prv: AnyOpaque,
) -> File {
    let root = root_pathref();
    let inode = Arc::new(Inode::new(
        Ino::try_new(ANONYMOUS_INO).unwrap(),
        mode.ty(),
        &ANONYMOUS_INODE_OPS,
        root.mount().sb().clone(),
        NilOpaque::new(),
    ));

    inode.set_meta(InodeMeta {
        nlink: 1,
        size: 0,
        perm: mode.perm(),
        atime: Duration::ZERO,
        mtime: Duration::ZERO,
        ctime: Duration::ZERO,
    });

    let inode = InodeRef::new(inode);
    let dentry = Arc::new(Dentry::new(
        name.as_ref().to_string(),
        Some(root.dentry()),
        inode,
    ));

    File::new(PathRef::new(root.mount().clone(), dentry), file_ops, prv)
}
