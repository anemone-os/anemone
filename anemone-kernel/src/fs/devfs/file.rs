use crate::{
    device::{
        block::{get_block_dev, next_block_dev, BlockDev},
        char::{get_char_dev, next_char_dev},
    },
    prelude::*,
    utils::iter_ctx::IterCtx,
};

use super::{devfs_ino_for, devfs_root_ino, DevfsNode};

#[derive(Debug, Clone, Copy, Opaque)]
pub(super) struct DevfsFile {
    node: DevfsNode,
}

impl DevfsFile {
    pub(super) fn new(node: DevfsNode) -> Self {
        Self { node }
    }

    fn node(self) -> DevfsNode {
        self.node
    }
}

fn devfs_file(file: &File) -> &DevfsFile {
    file.prv()
        .cast::<DevfsFile>()
        .expect("devfs file must carry DevfsFile private data")
}

fn count_char_devs() -> usize {
    let mut count = 0;
    let mut ctx = IterCtx::new();
    while next_char_dev(&mut ctx).is_some() {
        count += 1;
    }
    count
}

fn dir_iterate(file: &File, ctx: &mut DirContext) -> Result<DirEntry, FsError> {
    if devfs_file(file).node() != DevfsNode::Root {
        return Err(FsError::NotDir);
    }

    match ctx.offset() {
        0 => {
            ctx.advance(1);
            Ok(DirEntry {
                name: ".".to_string(),
                ino: devfs_root_ino(),
                ty: InodeType::Dir,
            })
        },
        1 => {
            ctx.advance(1);
            Ok(DirEntry {
                name: "..".to_string(),
                ino: devfs_root_ino(),
                ty: InodeType::Dir,
            })
        },
        _ => {
            let logical = ctx.offset() - 2;

            let mut cctx = IterCtx::with_offset(logical);
            if let Some(entry) = next_char_dev(&mut cctx) {
                ctx.advance(1);
                return Ok(DirEntry {
                    name: entry.name,
                    ino: devfs_ino_for(DevfsNode::Char(entry.devnum)),
                    ty: InodeType::Dev,
                });
            }

            let char_count = count_char_devs();
            let Some(block_offset) = logical.checked_sub(char_count) else {
                return Err(FsError::NoMoreEntries);
            };

            let mut bctx = IterCtx::with_offset(block_offset);
            let Some(entry) = next_block_dev(&mut bctx) else {
                return Err(FsError::NoMoreEntries);
            };

            ctx.advance(1);
            Ok(DirEntry {
                name: entry.name,
                ino: devfs_ino_for(DevfsNode::Block(entry.devnum)),
                ty: InodeType::Dev,
            })
        },
    }
}

fn char_read(file: &File, buf: &mut [u8]) -> Result<usize, FsError> {
    let DevfsNode::Char(devnum) = devfs_file(file).node() else {
        return Err(FsError::InvalidArgument);
    };

    get_char_dev(devnum).ok_or(FsError::NotFound)?.read(buf)
}

fn char_write(file: &File, buf: &[u8]) -> Result<usize, FsError> {
    let DevfsNode::Char(devnum) = devfs_file(file).node() else {
        return Err(FsError::InvalidArgument);
    };

    get_char_dev(devnum).ok_or(FsError::NotFound)?.write(buf)
}

fn char_seek(_file: &File, _pos: usize) -> Result<(), FsError> {
    Err(FsError::NotSupported)
}

fn block_dev_from_file(file: &File) -> Result<Arc<dyn BlockDev>, FsError> {
    let DevfsNode::Block(devnum) = devfs_file(file).node() else {
        return Err(FsError::InvalidArgument);
    };

    get_block_dev(devnum).ok_or(FsError::NotFound)
}

fn block_seek(file: &File, pos: usize) -> Result<(), FsError> {
    let dev = block_dev_from_file(file)?;
    let block_size = dev.block_size().bytes();
    let total_bytes = dev.total_blocks() * block_size;

    if pos % block_size != 0 || pos > total_bytes {
        return Err(FsError::InvalidArgument);
    }

    file.set_pos(pos);
    Ok(())
}

fn block_read(file: &File, buf: &mut [u8]) -> Result<usize, FsError> {
    if buf.is_empty() {
        return Ok(0);
    }

    let dev = block_dev_from_file(file)?;
    let block_size = dev.block_size().bytes();
    let pos = file.pos();
    let total_bytes = dev.total_blocks() * block_size;

    if pos % block_size != 0 || buf.len() % block_size != 0 {
        return Err(FsError::InvalidArgument);
    }

    if pos >= total_bytes {
        return Ok(0);
    }

    let nbytes = usize::min(buf.len(), total_bytes - pos);
    dev.read_blocks(pos / block_size, &mut buf[..nbytes])?;
    file.set_pos(pos + nbytes);
    Ok(nbytes)
}

fn block_write(file: &File, buf: &[u8]) -> Result<usize, FsError> {
    if buf.is_empty() {
        return Ok(0);
    }

    let dev = block_dev_from_file(file)?;
    let block_size = dev.block_size().bytes();
    let pos = file.pos();
    let total_bytes = dev.total_blocks() * block_size;

    if pos % block_size != 0 || buf.len() % block_size != 0 {
        return Err(FsError::InvalidArgument);
    }

    let Some(end_pos) = pos.checked_add(buf.len()) else {
        return Err(FsError::InvalidArgument);
    };
    if end_pos > total_bytes {
        return Err(FsError::NoSpace);
    }

    dev.write_blocks(pos / block_size, buf)?;
    file.set_pos(end_pos);
    Ok(buf.len())
}

pub(super) static DEVFS_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _| Err(FsError::IsDir),
    write: |_, _| Err(FsError::IsDir),
    seek: |_, _| Err(FsError::IsDir),
    iterate: dir_iterate,
};

pub(super) static DEVFS_CHAR_FILE_OPS: FileOps = FileOps {
    read: char_read,
    write: char_write,
    seek: char_seek,
    iterate: |_, _| Err(FsError::NotDir),
};

pub(super) static DEVFS_BLOCK_FILE_OPS: FileOps = FileOps {
    read: block_read,
    write: block_write,
    seek: block_seek,
    iterate: |_, _| Err(FsError::NotDir),
};
