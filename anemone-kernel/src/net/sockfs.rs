//! Anonymous-inode-backed socket filesystem ("sockfs").
//!
//! Every socket fd is a regular `FileDesc::Vfs` backed by an anonymous inode
//! of type `InodeType::Socket`.  I/O is dispatched through `SOCK_FILE_OPS`,
//! which forwards to the `user_socket_read` / `user_socket_write` helpers in
//! `net::user_socket`.  This mirrors the pipe implementation in `fs/pipe.rs`.

use alloc::sync::Arc;

use crate::{
    fs::anonymous::{anony_new_inode, anony_open_with},
    net::user_socket::{user_socket_read, user_socket_write, UserSocketShared},
    prelude::*,
    task::files::FileFlags,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

/// Private data carried by every socket `File`.
#[derive(Opaque)]
struct SockFilePrv {
    shared: Arc<UserSocketShared>,
    /// Snapshot of the file-open flags (e.g. `NONBLOCK`).  Kept here so that
    /// `SOCK_FILE_OPS` closures can inspect it without reaching back through
    /// the fd table.
    file_flags: FileFlags,
}

fn sock_get_attr(inode: &InodeRef) -> Result<InodeStat, FsError> {
    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: inode.nlink(),
        uid: 0,
        gid: 0,
        rdev: DeviceId::None,
        size: 0,
        atime: inode.atime(),
        mtime: inode.mtime(),
        ctime: inode.ctime(),
    })
}

static SOCK_INODE_OPS: InodeOps = InodeOps {
    lookup:    |_, _|    Err(FsError::NotDir),
    touch:     |_, _, _| Err(FsError::NotDir),
    mkdir:     |_, _, _| Err(FsError::NotDir),
    symlink:   |_, _, _| Err(FsError::NotDir),
    link:      |_, _, _| Err(FsError::NotDir),
    unlink:    |_, _|    Err(FsError::NotDir),
    rmdir:     |_, _|    Err(FsError::NotDir),
    open:      |_|       unreachable!(),
    read_link: |_|       Err(FsError::NotSymlink),
    get_attr:  sock_get_attr,
};

static SOCK_FILE_OPS: FileOps = FileOps {
    read: |file, buf| {
        let prv = file.prv().cast::<SockFilePrv>().expect("sock file must carry SockFilePrv");
        user_socket_read(&prv.shared, buf, prv.file_flags)
    },
    write: |file, buf| {
        let prv = file.prv().cast::<SockFilePrv>().expect("sock file must carry SockFilePrv");
        user_socket_write(&prv.shared, buf, prv.file_flags)
    },
    seek:    |_, _| Err(KernelError::NotSeekable.into()),
    iterate: |_, _| Err(KernelError::NotDirectory.into()),
};

/// Create an anonymous-inode-backed `File` representing a socket.
///
/// The returned `File` is ready to be installed into a task's fd table via
/// `Task::open_fd`.
pub fn create_socket_file(
    shared: Arc<UserSocketShared>,
    file_flags: FileFlags,
) -> Result<File, FsError> {
    let prv = AnyOpaque::new(SockFilePrv { shared, file_flags });
    let path = anony_new_inode(InodeType::Socket, &SOCK_INODE_OPS, NilOpaque::new())?;
    anony_open_with(&path, OpenedFile { file_ops: &SOCK_FILE_OPS, prv })
}

/// Extract the `Arc<UserSocketShared>` from a `FileDesc` that was created by
/// [`create_socket_file`].  Returns `None` if the fd does not hold socket
/// private data (i.e. it is not a socket fd).
pub fn get_socket_shared(fd: &FileDesc) -> Option<Arc<UserSocketShared>> {
    let file = fd.as_vfs_file()?;
    Some(file.prv().cast::<SockFilePrv>()?.shared.clone())
}
