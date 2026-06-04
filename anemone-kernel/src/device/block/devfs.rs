use crate::{
    fs::devfs::{DevfsNodeAttr, DevfsNodeOps, DevfsPublish, publish as devfs_publish},
    prelude::*,
    utils::any_opaque::NilOpaque,
};

use super::{BlockDev, get_block_dev, get_block_dev_name};

fn opened_block_file() -> OpenedFile {
    OpenedFile {
        file_ops: &BLOCK_DEV_FILE_OPS,
        prv: NilOpaque::new(),
    }
}

fn block_file_devnum(file: &File) -> Result<BlockDevNum, SysError> {
    match file.inode().get_attr()?.rdev {
        DeviceId::Block(devnum) => Ok(devnum),
        _ => Err(SysError::InvalidArgument),
    }
}

fn block_file_dev(file: &File) -> Result<Arc<dyn BlockDev>, SysError> {
    get_block_dev(block_file_devnum(file)?).ok_or(SysError::NotFound)
}

fn block_validate_seek(file: &File, pos: usize) -> Result<(), SysError> {
    let dev = block_file_dev(file)?;
    let block_size = dev.block_size().bytes();
    let total_bytes = dev.total_blocks() * block_size;

    if pos % block_size != 0 || pos > total_bytes {
        return Err(SysError::InvalidArgument);
    }

    Ok(())
}

// The block subsystem's default `/dev` behavior is still block-oriented: the
// cursor, buffer length, and device bounds must all stay block-aligned.
fn block_read(file: &File, pos: &mut usize, buf: &mut [u8]) -> Result<usize, SysError> {
    if buf.is_empty() {
        return Ok(0);
    }

    let old_pos = *pos;
    let dev = block_file_dev(file)?;
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
    let dev = block_file_dev(file)?;
    let block_size = dev.block_size().bytes();
    let total_bytes = dev.total_blocks() * block_size;

    if old_pos % block_size != 0 || buf.len() % block_size != 0 {
        return Err(SysError::InvalidArgument);
    }

    let Some(end_pos) = old_pos.checked_add(buf.len()) else {
        return Err(SysError::InvalidArgument);
    };
    if end_pos > total_bytes {
        return Err(SysError::NoSpace);
    }

    dev.write_blocks(old_pos / block_size, buf)?;
    *pos = end_pos;
    Ok(buf.len())
}

static BLOCK_DEV_FILE_OPS: FileOps = FileOps {
    read: block_read,
    write: block_write,
    validate_seek: block_validate_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    // Block devices also do not have a waitable poll path yet.
    poll: |_, _| Err(SysError::NotYetImplemented),
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

struct BlockDevFsNodeOps {
    devnum: BlockDevNum,
}

impl DevfsNodeOps for BlockDevFsNodeOps {
    fn open(&self, _inode: &InodeRef) -> Result<OpenedFile, SysError> {
        get_block_dev(self.devnum).ok_or(SysError::NotFound)?;
        Ok(opened_block_file())
    }

    fn get_attr(&self, inode: &InodeRef, attr: DevfsNodeAttr) -> Result<InodeStat, SysError> {
        let dev = get_block_dev(self.devnum).ok_or(SysError::NotFound)?;

        Ok(InodeStat {
            fs_dev: DeviceId::None,
            ino: inode.ino(),
            mode: InodeMode::new(attr.ty, inode.perm()),
            nlink: inode.nlink(),
            uid: inode.uid(),
            gid: inode.gid(),
            rdev: attr.rdev,
            size: (dev.block_size().bytes() * dev.total_blocks()) as u64,
            atime: inode.atime(),
            mtime: inode.mtime(),
            ctime: inode.ctime(),
        })
    }
}

// The block subsystem owns the default `/dev` behavior for block devices.
// devfs only stores the publish record and dispatches into this helper.
pub fn publish_block_device(devnum: BlockDevNum) -> Result<Ino, SysError> {
    let name = get_block_dev_name(devnum).ok_or(SysError::NotFound)?;

    devfs_publish(DevfsPublish {
        name,
        attr: DevfsNodeAttr {
            ty: InodeType::Block,
            perm: InodePerm::all_rw(),
            rdev: DeviceId::Block(devnum),
        },
        ops: Arc::new(BlockDevFsNodeOps { devnum }),
    })
}
