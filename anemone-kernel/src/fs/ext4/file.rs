use core::str;

use crate::{
    fs::{
        UserBufferSink,
        address_space::{AddressSpace, AddressSpaceBackend},
        ext4::{ext4_ino, ext4_sb, map_ext4_error, map_lwext4_inode_type},
        iomux::PollEvent,
        uio::UserBufferSource,
    },
    prelude::*,
};

static_assert!(
    EXT4_SYNC_IO_BATCH_PAGES > 0,
    "ext4_sync_io_batch_pages must be non-zero"
);
static_assert!(
    EXT4_SYNC_IO_BATCH_PAGES <= isize::MAX as usize / PagingArch::PAGE_SIZE_BYTES,
    "ext4 synchronous I/O batch buffer must fit the allocation size domain"
);

/// Ext4 identity and transaction access for one inode address space.
///
/// This capability deliberately has no page map, dirty state, logical-size
/// mirror, inode reference, or user buffer access.
pub(super) struct Ext4AddressSpaceBackend {
    ino: Ino,
    sb: Arc<SuperBlock>,
}

impl Ext4AddressSpaceBackend {
    pub(super) fn new(sb: Arc<SuperBlock>, ino: Ino) -> Self {
        Self { ino, sb }
    }
}

impl AddressSpaceBackend for Ext4AddressSpaceBackend {
    fn batch_page_cap(&self) -> usize {
        EXT4_SYNC_IO_BATCH_PAGES
    }

    fn fill_range(&self, offset: usize, data: &mut [u8]) -> Result<(), SysError> {
        ext4_sb(&self.sb).with_fs(|fs| {
            fs.read_at(self.ino.get() as u32, data, offset as u64)
                .map(|_| ())
                .map_err(map_ext4_error)
        })
    }

    fn writeback_range(&self, offset: usize, data: &[u8]) -> Result<(), SysError> {
        ext4_sb(&self.sb).with_fs(|fs| {
            fs.write_at(self.ino.get() as u32, data, offset as u64)
                .map(|_| ())
                .map_err(|err| {
                    kwarningln!(
                        "ext4: failed to write range at offset {} of inode {}: {:?}",
                        offset,
                        self.ino.get(),
                        err
                    );
                    SysError::InvalidArgument
                })
        })
    }
}

fn ext4_address_space(inode: &InodeRef) -> Result<&Arc<AddressSpace>, SysError> {
    if inode.ty() != InodeType::Regular {
        return Err(SysError::NotReg);
    }
    Ok(inode
        .inode()
        .address_space()
        .expect("regular ext4 inode must own an address space"))
}

fn ext4_read(
    file: &File,
    pos: &mut usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let inode = file.inode();
    let address_space = ext4_address_space(inode)?;
    let size = usize::try_from(inode.size()).map_err(|_| SysError::FileTooLarge)?;
    if *pos >= size {
        return Ok(0);
    }

    let n = buf.len().min(size - *pos);
    address_space.read(*pos, &mut buf[..n])?;
    *pos += n;
    Ok(n)
}

fn ext4_read_at(
    file: &File,
    pos: usize,
    buf: &mut [u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let mut local_pos = pos;
    ext4_read(file, &mut local_pos, buf, ctx)
}

fn ext4_read_user_at(
    file: &File,
    pos: usize,
    dst: &mut UserBufferSink<'_>,
    _ctx: FileIoCtx,
) -> Result<(), SysError> {
    let inode = file.inode();
    let address_space = ext4_address_space(inode)?;
    let size = usize::try_from(inode.size()).map_err(|_| SysError::FileTooLarge)?;
    if pos >= size {
        return Ok(());
    }

    address_space.read_user(pos, dst.remaining().min(size - pos), dst)
}

fn ext4_write(
    file: &File,
    pos: &mut usize,
    buf: &[u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let inode = file.inode();
    let address_space = ext4_address_space(inode)?;
    let new_pos = address_space.write(*pos, buf)?;
    inode.inode().update_size_max(new_pos as u64);
    *pos = new_pos;
    Ok(buf.len())
}

fn ext4_write_at(file: &File, pos: usize, buf: &[u8], ctx: FileIoCtx) -> Result<usize, SysError> {
    let mut local_pos = pos;
    ext4_write(file, &mut local_pos, buf, ctx)
}

fn ext4_write_user_at(
    file: &File,
    pos: usize,
    src: &mut UserBufferSource<'_>,
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let inode = file.inode();
    let address_space = ext4_address_space(inode)?;
    let len = src.remaining();
    let _ = pos.checked_add(len).ok_or(SysError::InvalidArgument)?;
    let written = address_space.write_user(pos, len, src)?;
    if written > 0 {
        let new_end = pos.checked_add(written).ok_or(SysError::InvalidArgument)?;
        inode.inode().update_size_max(new_end as u64);
    }
    Ok(written)
}

fn ext4_seek(file: &File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError> {
    seek_with_inode_size(file, pos, from)
}

fn ext4_read_dir(
    file: &File,
    offset: &mut usize,
    sink: &mut dyn DirSink,
) -> Result<ReadDirResult, SysError> {
    let inode = file.inode();
    if inode.ty() != InodeType::Dir {
        return Err(SysError::NotDir);
    }

    let sb = inode.sb();
    let mut pushed_any = false;
    loop {
        let entry = ext4_sb(&sb).with_fs(|fs| {
            let mut reader = fs
                .read_dir(inode.ino().get() as u32, *offset as u64)
                .map_err(map_ext4_error)?;
            let Some(current) = reader.current() else {
                return Ok(None);
            };
            let entry = DirEntry {
                name: str::from_utf8(current.name())
                    .map_err(|_| SysError::InvalidArgument)?
                    .to_string(),
                ino: ext4_ino(current.ino())?,
                ty: map_lwext4_inode_type(current.inode_type())?,
            };
            reader.step().map_err(map_ext4_error)?;
            Ok(Some((entry, reader.offset() as usize)))
        })?;

        let Some((entry, next_offset)) = entry else {
            return if pushed_any {
                Ok(ReadDirResult::Progressed)
            } else {
                Ok(ReadDirResult::Eof)
            };
        };
        match sink.push(entry)? {
            SinkResult::Accepted => {
                pushed_any = true;
                *offset = next_offset;
            },
            SinkResult::Stop => return Ok(ReadDirResult::Progressed),
        }
    }
}

pub(super) static EXT4_REG_FILE_OPS: FileOps = FileOps {
    read: ext4_read,
    write: ext4_write,
    read_at: ext4_read_at,
    write_at: ext4_write_at,
    read_user_at: Some(ext4_read_user_at),
    write_user_at: Some(ext4_write_user_at),
    check_status_flags: accept_file_op_status_flags,
    seek: ext4_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, req| {
        Ok(req.ready_or_unsupported((PollEvent::READABLE | PollEvent::WRITABLE) & req.interests()))
    },
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

pub(super) static EXT4_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::IsDir),
    write: |_, _, _, _| Err(SysError::IsDir),
    read_at: |_, _, _, _| Err(SysError::IsDir),
    write_at: |_, _, _, _| Err(SysError::IsDir),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: seek_dir_rewind,
    read_dir: ext4_read_dir,
    poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

pub(super) static EXT4_SYMLINK_FILE_OPS: FileOps = FileOps {
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
