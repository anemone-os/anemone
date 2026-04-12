use core::str;

use crate::{
    fs::ext4::{ext4_ino, ext4_sb, map_ext4_error, map_lwext4_inode_type},
    prelude::*,
};

#[derive(Opaque)]
pub(super) struct Ext4File {
    _data: (),
}

impl Ext4File {
    pub(super) fn new() -> Self {
        Self { _data: () }
    }
}

fn ext4_read(file: &File, buf: &mut [u8]) -> Result<usize, FsError> {
    let inode = file.inode();
    if inode.ty() != InodeType::Regular {
        return Err(FsError::NotReg);
    }

    let pos = file.pos();
    let sb = inode.sb();
    let n = ext4_sb(&sb).read_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            fs.read_at(inode.ino().get() as u32, buf, pos as u64)
                .map_err(map_ext4_error)
        })
    })?;
    file.set_pos(pos + n);
    Ok(n)
}

fn ext4_write(file: &File, buf: &[u8]) -> Result<usize, FsError> {
    let inode = file.inode();
    if inode.ty() != InodeType::Regular {
        return Err(FsError::NotReg);
    }

    let pos = file.pos();
    let sb = inode.sb();
    let n = ext4_sb(&sb).write_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            fs.write_at(inode.ino().get() as u32, buf, pos as u64)
                .map_err(map_ext4_error)
        })
    })?;
    inode.inode().update_size_max((pos + n) as u64);
    file.set_pos(pos + n);
    Ok(n)
}

fn ext4_seek(file: &File, pos: usize) -> Result<(), FsError> {
    file.set_pos(pos);
    Ok(())
}

fn ext4_iterate(file: &File, ctx: &mut DirContext) -> Result<DirEntry, FsError> {
    let inode = file.inode();
    if inode.ty() != InodeType::Dir {
        return Err(FsError::NotDir);
    }

    let sb = inode.sb();
    let start = ctx.offset() as u64;
    let (advance, name, ino, ty) = ext4_sb(&sb).read_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            let mut reader = fs
                .read_dir(inode.ino().get() as u32, start)
                .map_err(map_ext4_error)?;
            let current = reader.current().ok_or(FsError::NoMoreEntries)?;
            let cur_off = reader.offset();
            let name = str::from_utf8(current.name())
                .map_err(|_| FsError::InvalidArgument)?
                .to_string();
            let ino = ext4_ino(current.ino())?;
            let ty = map_lwext4_inode_type(current.inode_type())?;
            reader.step().map_err(map_ext4_error)?;
            let next_off = reader.offset();
            // todo?
            let advance = if next_off > cur_off {
                (next_off - cur_off) as usize
            } else {
                1
            };
            Ok((advance, name, ino, ty))
        })
    })?;
    ctx.advance(advance);

    Ok(DirEntry { name, ino, ty })
}

pub(super) static EXT4_REG_FILE_OPS: FileOps = FileOps {
    read: ext4_read,
    write: ext4_write,
    seek: ext4_seek,
    iterate: |_, _| Err(FsError::NotDir),
};

pub(super) static EXT4_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _| Err(FsError::IsDir),
    write: |_, _| Err(FsError::IsDir),
    seek: |_, _| Err(FsError::IsDir),
    iterate: ext4_iterate,
};

pub(super) static EXT4_SYMLINK_FILE_OPS: FileOps = FileOps {
    read: |_, _| Err(FsError::NotSupported),
    write: |_, _| Err(FsError::NotSupported),
    seek: |_, _| Err(FsError::NotSupported),
    iterate: |_, _| Err(FsError::NotDir),
};
