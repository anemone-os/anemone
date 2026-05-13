use crate::{
    device::{
        block::{BlockDev, get_block_dev, next_block_dev},
        char::{get_char_dev, next_char_dev},
    },
    fs::iomux::PollEvent,
    prelude::*,
    utils::iter_ctx::IterCtx,
};

use super::{DevfsNode, devfs_ino_for, devfs_root_ino};

const DEVFS_DOT_CURSOR: usize = 0;
const DEVFS_DOTDOT_CURSOR: usize = 1;
const DEVFS_CHAR_CURSOR_BASE: usize = 2;
const DEVFS_BLOCK_CURSOR_BASE: usize = 1usize << (usize::BITS as usize - 1);

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

fn push_root_entry(
    sink: &mut dyn DirSink,
    name: &str,
    ino: Ino,
    ty: InodeType,
) -> Result<SinkResult, SysError> {
    sink.push(DirEntry {
        name: name.to_string(),
        ino,
        ty,
    })
}

fn dir_read_dir(
    file: &File,
    pos: &mut usize,
    sink: &mut dyn DirSink,
) -> Result<ReadDirResult, SysError> {
    if devfs_file(file).node() != DevfsNode::Root {
        return Err(SysError::NotDir);
    }

    let mut pushed_any = false;

    loop {
        match *pos {
            DEVFS_DOT_CURSOR => {
                match push_root_entry(sink, ".", devfs_root_ino(), InodeType::Dir)? {
                    SinkResult::Accepted => {
                        pushed_any = true;
                        *pos = DEVFS_DOTDOT_CURSOR;
                    },
                    SinkResult::Stop => return Ok(ReadDirResult::Progressed),
                }
            },
            DEVFS_DOTDOT_CURSOR => {
                match push_root_entry(sink, "..", devfs_root_ino(), InodeType::Dir)? {
                    SinkResult::Accepted => {
                        pushed_any = true;
                        *pos = DEVFS_CHAR_CURSOR_BASE;
                    },
                    SinkResult::Stop => return Ok(ReadDirResult::Progressed),
                }
            },
            block_pos if block_pos >= DEVFS_BLOCK_CURSOR_BASE => {
                let mut ctx = IterCtx::with_offset(block_pos - DEVFS_BLOCK_CURSOR_BASE);
                let Some(entry) = next_block_dev(&mut ctx) else {
                    return if pushed_any {
                        Ok(ReadDirResult::Progressed)
                    } else {
                        Ok(ReadDirResult::Eof)
                    };
                };

                match sink.push(DirEntry {
                    name: entry.name,
                    ino: devfs_ino_for(DevfsNode::Block(entry.devnum)),
                    ty: InodeType::Block,
                })? {
                    SinkResult::Accepted => {
                        pushed_any = true;
                        *pos = DEVFS_BLOCK_CURSOR_BASE + ctx.cur_offset();
                    },
                    SinkResult::Stop => return Ok(ReadDirResult::Progressed),
                }
            },
            _ => {
                let mut ctx = IterCtx::with_offset(*pos - DEVFS_CHAR_CURSOR_BASE);
                if let Some(entry) = next_char_dev(&mut ctx) {
                    match sink.push(DirEntry {
                        name: entry.name,
                        ino: devfs_ino_for(DevfsNode::Char(entry.devnum)),
                        ty: InodeType::Char,
                    })? {
                        SinkResult::Accepted => {
                            pushed_any = true;
                            *pos = DEVFS_CHAR_CURSOR_BASE + ctx.cur_offset();
                        },
                        SinkResult::Stop => return Ok(ReadDirResult::Progressed),
                    }
                } else {
                    *pos = DEVFS_BLOCK_CURSOR_BASE;
                }
            },
        }
    }
}

fn char_read(file: &File, _pos: &mut usize, buf: &mut [u8]) -> Result<usize, SysError> {
    let DevfsNode::Char(devnum) = devfs_file(file).node() else {
        return Err(SysError::InvalidArgument);
    };

    get_char_dev(devnum).ok_or(SysError::NotFound)?.read(buf)
}

fn char_write(file: &File, _pos: &mut usize, buf: &[u8]) -> Result<usize, SysError> {
    let DevfsNode::Char(devnum) = devfs_file(file).node() else {
        return Err(SysError::InvalidArgument);
    };

    get_char_dev(devnum).ok_or(SysError::NotFound)?.write(buf)
}

fn block_dev_from_file(file: &File) -> Result<Arc<dyn BlockDev>, SysError> {
    let DevfsNode::Block(devnum) = devfs_file(file).node() else {
        return Err(SysError::InvalidArgument);
    };

    get_block_dev(devnum).ok_or(SysError::NotFound)
}

fn block_validate_seek(file: &File, pos: usize) -> Result<(), SysError> {
    let dev = block_dev_from_file(file)?;
    let block_size = dev.block_size().bytes();
    let total_bytes = dev.total_blocks() * block_size;

    if pos % block_size != 0 || pos > total_bytes {
        return Err(SysError::InvalidArgument);
    }

    Ok(())
}

fn block_read(file: &File, pos: &mut usize, buf: &mut [u8]) -> Result<usize, SysError> {
    if buf.is_empty() {
        return Ok(0);
    }

    let old_pos = *pos;

    let dev = block_dev_from_file(file)?;
    let block_size = dev.block_size().bytes();
    let total_bytes = dev.total_blocks() * block_size;

    if old_pos % block_size != 0 || buf.len() % block_size != 0 {
        return Err(SysError::InvalidArgument);
    }

    if old_pos >= total_bytes {
        return Ok(0);
    }

    let nbytes = usize::min(buf.len(), total_bytes - old_pos);
    dev.read_blocks(old_pos / block_size, &mut buf[..nbytes])?;
    *pos = old_pos + nbytes;
    Ok(nbytes)
}

fn block_write(file: &File, pos: &mut usize, buf: &[u8]) -> Result<usize, SysError> {
    if buf.is_empty() {
        return Ok(0);
    }

    let old_pos = *pos;

    let dev = block_dev_from_file(file)?;
    let block_size = dev.block_size().bytes();
    let total_bytes = dev.total_blocks() * block_size;

    if old_pos % block_size != 0 || buf.len() % block_size != 0 {
        return Err(SysError::InvalidArgument);
    }

    let Some(end_pos) = pos.checked_add(buf.len()) else {
        return Err(SysError::InvalidArgument);
    };
    if end_pos > total_bytes {
        return Err(SysError::NoSpace);
    }

    dev.write_blocks(old_pos / block_size, buf)?;
    *pos = end_pos;
    Ok(buf.len())
}

pub(super) static DEVFS_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _, _| Err(SysError::IsDir),
    write: |_, _, _| Err(SysError::IsDir),
    validate_seek: |_, _| Err(SysError::IsDir),
    read_dir: dir_read_dir,
    poll: |_, _| Ok(PollEvent::READABLE),
};

pub(super) static DEVFS_CHAR_FILE_OPS: FileOps = FileOps {
    read: char_read,
    write: char_write,
    validate_seek: |_, _| Err(SysError::NotSupported),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, _| todo!("we should refactor devfs."),
};

pub(super) static DEVFS_BLOCK_FILE_OPS: FileOps = FileOps {
    read: block_read,
    write: block_write,
    validate_seek: block_validate_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, _| Ok(PollEvent::READABLE | PollEvent::WRITABLE),
};
