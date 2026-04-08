//! In current implementation this is not a real filesystem. It just leverages
//! anonymous inodes to create pipes.

use crate::{prelude::*, utils::any_opaque::NilOpaque};

#[derive(Opaque)]
struct Pipe {}

static PIPE_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(FsError::NotDir),
    create: |_, _, _| Err(FsError::NotDir),
    symlink: |_, _, _| Err(FsError::NotDir),
    link: |_, _, _| Err(FsError::NotDir),
    unlink: |_, _| Err(FsError::NotDir),
    rmdir: |_, _| Err(FsError::NotDir),
    open: |_| unreachable!(/* pipes have their own open logic */),
    read_link: |_| Err(FsError::NotSymlink),
    get_attr: |_| todo!(),
};

#[derive(Opaque)]
struct PipeRx {}

static PIPE_RX_FILE_OPS: FileOps = FileOps {
    read: |_, _| todo!(),
    write: |_, _| Err(FsError::NotSupported),
    seek: |_, _| Err(FsError::NotSupported),
    iterate: |_, _| Err(FsError::NotDir),
};

#[derive(Opaque)]
struct PipeTx {}

static PIPE_TX_FILE_OPS: FileOps = FileOps {
    read: |_, _| Err(FsError::NotSupported),
    write: |_, _| todo!(),
    seek: |_, _| Err(FsError::NotSupported),
    iterate: |_, _| Err(FsError::NotDir),
};

pub struct OpenedPipe {
    pub rx: File,
    pub tx: File,
}

/// Creates an anonymous pipe and returns the read and write ends of it.
pub fn create_anonymous_pipe() -> Result<OpenedPipe, FsError> {
    let inode = anony_new_inode(InodeType::Fifo, &PIPE_INODE_OPS, NilOpaque::new())?;

    todo!()
}

// TODO: named pipes. i.e. fifo. we'll do this after we refactor current inode
// ops vtable.
