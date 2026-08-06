use crate::{
    fs::{
        UserBufferSink, address_space::AddressSpace, iomux::PollEvent, ramfs::ramfs_dir,
        uio::UserBufferSource,
    },
    prelude::*,
};

fn ramfs_address_space(inode: &InodeRef) -> Result<&Arc<AddressSpace>, SysError> {
    if inode.ty() != InodeType::Regular {
        return Err(SysError::NotReg);
    }
    Ok(inode
        .inode()
        .address_space()
        .expect("regular ramfs inode must own an address space"))
}

fn ramfs_read(
    file: &File,
    pos: &mut usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let inode = file.inode();
    let address_space = ramfs_address_space(inode)?;
    let size = usize::try_from(inode.size()).map_err(|_| SysError::FileTooLarge)?;
    if *pos >= size {
        return Ok(0);
    }

    let n = buf.len().min(size - *pos);
    address_space.read(*pos, &mut buf[..n])?;
    *pos += n;
    Ok(n)
}

fn ramfs_read_at(
    file: &File,
    pos: usize,
    buf: &mut [u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let mut local_pos = pos;
    ramfs_read(file, &mut local_pos, buf, ctx)
}

fn ramfs_read_user_at(
    file: &File,
    pos: usize,
    dst: &mut UserBufferSink<'_>,
    _ctx: FileIoCtx,
) -> Result<(), SysError> {
    let inode = file.inode();
    let address_space = ramfs_address_space(inode)?;
    let size = usize::try_from(inode.size()).map_err(|_| SysError::FileTooLarge)?;
    if pos >= size {
        return Ok(());
    }

    address_space.read_user(pos, dst.remaining().min(size - pos), dst)
}

fn ramfs_write(
    file: &File,
    pos: &mut usize,
    buf: &[u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let inode = file.inode();
    let address_space = ramfs_address_space(inode)?;
    let new_pos = address_space.write(*pos, buf)?;
    inode.inode().update_size_max(new_pos as u64);
    *pos = new_pos;
    Ok(buf.len())
}

fn ramfs_write_at(file: &File, pos: usize, buf: &[u8], ctx: FileIoCtx) -> Result<usize, SysError> {
    let mut local_pos = pos;
    ramfs_write(file, &mut local_pos, buf, ctx)
}

fn ramfs_write_user_at(
    file: &File,
    pos: usize,
    src: &mut UserBufferSource<'_>,
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let inode = file.inode();
    let address_space = ramfs_address_space(inode)?;
    let len = src.remaining();
    let _ = pos.checked_add(len).ok_or(SysError::InvalidArgument)?;
    let written = address_space.write_user(pos, len, src)?;
    if written > 0 {
        let new_end = pos.checked_add(written).ok_or(SysError::InvalidArgument)?;
        inode.inode().update_size_max(new_end as u64);
    }
    Ok(written)
}

fn ramfs_seek(file: &File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError> {
    let base = match from {
        SeekFrom::End(_) => {
            usize::try_from(file.inode().size()).map_err(|_| SysError::FileTooLarge)?
        },
        _ => 0,
    };

    // Seeking beyond EOF is allowed; the gap is zero-filled on the next write.
    seek_with_fixed_size(file, pos, from, base)
}

fn ramfs_read_dir(
    file: &File,
    offset: &mut usize,
    sink: &mut dyn DirSink,
) -> Result<ReadDirResult, SysError> {
    let inode = file.inode();
    let dir_data = ramfs_dir(inode)?;
    let mut pushed_any = false;

    loop {
        let entry = dir_data.get_by_offset(*offset);
        if let Some((name, ino)) = entry {
            let ty = inode.sb().iget(ino)?.ty();
            match sink.push(DirEntry { name, ino, ty })? {
                SinkResult::Accepted => {
                    pushed_any = true;
                    *offset += 1;
                },
                SinkResult::Stop => break Ok(ReadDirResult::Progressed),
            }
        } else if pushed_any {
            return Ok(ReadDirResult::Progressed);
        } else {
            return Ok(ReadDirResult::Eof);
        }
    }
}

pub(super) static RAMFS_REG_FILE_OPS: FileOps = FileOps {
    read: ramfs_read,
    write: ramfs_write,
    read_at: ramfs_read_at,
    write_at: ramfs_write_at,
    read_user_at: Some(ramfs_read_user_at),
    write_user_at: Some(ramfs_write_user_at),
    check_status_flags: accept_file_op_status_flags,
    seek: ramfs_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, req| {
        Ok(req.ready_or_unsupported((PollEvent::READABLE | PollEvent::WRITABLE) & req.interests()))
    },
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

pub(super) static RAMFS_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::IsDir),
    write: |_, _, _, _| Err(SysError::IsDir),
    read_at: |_, _, _, _| Err(SysError::IsDir),
    write_at: |_, _, _, _| Err(SysError::IsDir),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: seek_dir_rewind,
    read_dir: ramfs_read_dir,
    poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

pub(super) static RAMFS_SYMLINK_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::NotSupported),
    write: |_, _, _, _| Err(SysError::NotSupported),
    read_at: |_, _, _, _| Err(SysError::NotSupported),
    write_at: |_, _, _, _| Err(SysError::NotSupported),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: |_, _, _| Err(SysError::NotSupported),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};
