use crate::{
    fs::devfs::{DevfsNodeAttr, DevfsNodeOps, DevfsPublish, publish as devfs_publish},
    prelude::*,
    utils::any_opaque::NilOpaque,
};

use super::{get_char_dev, get_char_dev_name};

fn opened_char_file() -> OpenedFile {
    OpenedFile {
        file_ops: &CHAR_DEV_FILE_OPS,
        prv: NilOpaque::new(),
    }
}

fn char_file_devnum(file: &File) -> Result<CharDevNum, SysError> {
    match file.inode().get_attr()?.rdev {
        DeviceId::Char(devnum) => Ok(devnum),
        _ => Err(SysError::InvalidArgument),
    }
}

fn char_file_read(file: &File, _pos: &mut usize, buf: &mut [u8]) -> Result<usize, SysError> {
    get_char_dev(char_file_devnum(file)?)
        .ok_or(SysError::NotFound)?
        .read(buf)
}

fn char_file_write(file: &File, _pos: &mut usize, buf: &[u8]) -> Result<usize, SysError> {
    get_char_dev(char_file_devnum(file)?)
        .ok_or(SysError::NotFound)?
        .write(buf)
}

static CHAR_DEV_FILE_OPS: FileOps = FileOps {
    read: char_file_read,
    write: char_file_write,
    validate_seek: |_, _| Err(SysError::NotSupported),
    read_dir: |_, _, _| Err(SysError::NotDir),
    // Char devices do not have a waitable poll path yet. Report NYI instead of
    // pretending every device is immediately readable or writable.
    poll: |_, _| Err(SysError::NotYetImplemented),
};

struct CharDevFsNodeOps {
    devnum: CharDevNum,
}

impl DevfsNodeOps for CharDevFsNodeOps {
    fn open(&self, _inode: &InodeRef) -> Result<OpenedFile, SysError> {
        get_char_dev(self.devnum).ok_or(SysError::NotFound)?;
        Ok(opened_char_file())
    }

    fn get_attr(&self, inode: &InodeRef, attr: DevfsNodeAttr) -> Result<InodeStat, SysError> {
        get_char_dev(self.devnum).ok_or(SysError::NotFound)?;

        Ok(InodeStat {
            fs_dev: DeviceId::None,
            ino: inode.ino(),
            mode: InodeMode::new(attr.ty, inode.perm()),
            nlink: inode.nlink(),
            uid: 0,
            gid: 0,
            rdev: attr.rdev,
            size: inode.size(),
            atime: inode.atime(),
            mtime: inode.mtime(),
            ctime: inode.ctime(),
        })
    }
}

// The char subsystem owns the default `/dev` behavior for character devices.
// devfs only stores the publish record and dispatches into this helper.
pub fn publish_char_device(devnum: CharDevNum) -> Result<Ino, SysError> {
    let name = get_char_dev_name(devnum).ok_or(SysError::NotFound)?;

    devfs_publish(DevfsPublish {
        name,
        attr: DevfsNodeAttr {
            ty: InodeType::Char,
            perm: InodePerm::all_rw(),
            rdev: DeviceId::Char(devnum),
        },
        ops: Arc::new(CharDevFsNodeOps { devnum }),
    })
}