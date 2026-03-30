use crate::{
    fs::ramfs::{ramfs_dir, ramfs_reg},
    prelude::*,
};

#[derive(Opaque)]
pub(super) struct RamfsFile {
    // it seems ramfs does not need any private data for opened files.
    _data: (),
}

impl RamfsFile {
    pub(super) fn new() -> Self {
        Self { _data: () }
    }
}

fn ramfs_read(file: &File, buf: &mut [u8]) -> Result<usize, FsError> {
    let inode = file.inode();
    let reg_data = ramfs_reg(inode)?;
    let data = reg_data.data.read_irqsave();

    let pos = file.pos();
    if pos >= data.len() {
        return Ok(0); // EOF
    }

    let n = usize::min(buf.len(), data.len() - pos);
    buf[..n].copy_from_slice(&data[pos..pos + n]);
    file.set_pos(pos + n);

    Ok(n)
}

fn ramfs_write(file: &File, buf: &[u8]) -> Result<usize, FsError> {
    let inode = file.inode();
    let reg_data = ramfs_reg(inode)?;
    let mut data = reg_data.data.write_irqsave();

    let pos = file.pos();
    if pos > data.len() {
        // zero-fill the gap
        data.resize(pos, 0);
    }

    if pos == data.len() {
        // append
        data.extend_from_slice(buf);
    } else {
        // overwrite
        let end_pos = usize::min(pos + buf.len(), data.len());
        data[pos..end_pos].copy_from_slice(&buf[..end_pos - pos]);
        if end_pos < pos + buf.len() {
            // append the remaining part
            data.extend_from_slice(&buf[end_pos - pos..]);
        }
    }

    inode.inode().set_size(data.len() as u64);
    file.set_pos(pos + buf.len());
    Ok(buf.len())
}

fn ramfs_seek(file: &File, pos: usize) -> Result<(), FsError> {
    // allow seeking beyond EOF; the gap will be zero-filled on the next write.
    file.set_pos(pos);
    Ok(())
}

fn ramfs_iterate(file: &File, ctx: &mut DirContext) -> Result<DirEntry, FsError> {
    let inode = file.inode();
    let dir_data = ramfs_dir(inode)?;

    let entry = dir_data.get_by_offset(ctx.offset());
    if entry.is_none() {
        return Err(FsError::NoMoreEntries);
    }
    let (name, ino) = entry.unwrap();

    ctx.advance(1);

    let inode = inode.sb().iget(ino).expect("ino exists but failed to load");

    Ok(DirEntry {
        name,
        ino,
        ty: inode.ty(),
    })
}

pub(super) static RAMFS_REG_FILE_OPS: FileOps = FileOps {
    read: ramfs_read,
    write: ramfs_write,
    seek: ramfs_seek,
    iterate: |_, _| Err(FsError::NotDir),
};

pub(super) static RAMFS_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _| Err(FsError::IsDir),
    write: |_, _| Err(FsError::IsDir),
    seek: |_, _| Err(FsError::IsDir),
    iterate: ramfs_iterate,
};
