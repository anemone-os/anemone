use crate::prelude::*;

use super::{published_node_at, DEVFS_ROOT_INO};

const DEVFS_DOT_CURSOR: usize = 0;
const DEVFS_DOTDOT_CURSOR: usize = 1;
const DEVFS_ENTRY_CURSOR_BASE: usize = 2;

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

fn devfs_root_read_dir(
    _file: &File,
    pos: &mut usize,
    sink: &mut dyn DirSink,
) -> Result<ReadDirResult, SysError> {
    let mut pushed_any = false;

    loop {
        match *pos {
            DEVFS_DOT_CURSOR => match push_root_entry(sink, ".", DEVFS_ROOT_INO, InodeType::Dir)? {
                SinkResult::Accepted => {
                    pushed_any = true;
                    *pos = DEVFS_DOTDOT_CURSOR;
                },
                SinkResult::Stop => {
                    return Ok(if pushed_any {
                        ReadDirResult::Progressed
                    } else {
                        ReadDirResult::Eof
                    });
                },
            },
            DEVFS_DOTDOT_CURSOR => {
                match push_root_entry(sink, "..", DEVFS_ROOT_INO, InodeType::Dir)? {
                    SinkResult::Accepted => {
                        pushed_any = true;
                        *pos = DEVFS_ENTRY_CURSOR_BASE;
                    },
                    SinkResult::Stop => {
                        return Ok(if pushed_any {
                            ReadDirResult::Progressed
                        } else {
                            ReadDirResult::Eof
                        });
                    },
                }
            },
            _ => break,
        }
    }

    // Static devfs nodes are enumerated in publish order. Since this first cut
    // only supports early static publish, the cursor can stay a plain index.
    while let Some(node) = published_node_at(*pos - DEVFS_ENTRY_CURSOR_BASE) {
        match sink.push(DirEntry {
            name: node.name.clone(),
            ino: node.ino,
            ty: node.attr.ty,
        })? {
            SinkResult::Accepted => {
                pushed_any = true;
                *pos += 1;
            },
            SinkResult::Stop => return Ok(ReadDirResult::Progressed),
        }
    }

    Ok(if pushed_any {
        ReadDirResult::Progressed
    } else {
        ReadDirResult::Eof
    })
}

fn devfs_dir_read_dir(
    file: &File,
    pos: &mut usize,
    sink: &mut dyn DirSink,
) -> Result<ReadDirResult, SysError> {
    let mut pushed_any = false;
    let self_ino = file.inode().ino();

    loop {
        match *pos {
            DEVFS_DOT_CURSOR => match push_root_entry(sink, ".", self_ino, InodeType::Dir)? {
                SinkResult::Accepted => {
                    pushed_any = true;
                    *pos = DEVFS_DOTDOT_CURSOR;
                },
                SinkResult::Stop => {
                    return Ok(if pushed_any {
                        ReadDirResult::Progressed
                    } else {
                        ReadDirResult::Eof
                    });
                },
            },
            DEVFS_DOTDOT_CURSOR => {
                match push_root_entry(sink, "..", DEVFS_ROOT_INO, InodeType::Dir)? {
                    SinkResult::Accepted => {
                        pushed_any = true;
                        *pos = DEVFS_ENTRY_CURSOR_BASE;
                    },
                    SinkResult::Stop => {
                        return Ok(if pushed_any {
                            ReadDirResult::Progressed
                        } else {
                            ReadDirResult::Eof
                        });
                    },
                }
            },
            _ => break,
        }
    }

    Ok(if pushed_any {
        ReadDirResult::Progressed
    } else {
        ReadDirResult::Eof
    })
}

pub(super) static DEVFS_ROOT_FILE_OPS: FileOps = FileOps {
    read: |_, _, _| Err(SysError::IsDir),
    write: |_, _, _| Err(SysError::IsDir),
    validate_seek: |_, _| Err(SysError::IsDir),
    read_dir: devfs_root_read_dir,
    // We do not have a real poll story for pseudo directories yet.
    poll: |_, _| Err(SysError::NotYetImplemented),
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

pub(super) static DEVFS_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _, _| Err(SysError::IsDir),
    write: |_, _, _| Err(SysError::IsDir),
    validate_seek: |_, _| Err(SysError::IsDir),
    read_dir: devfs_dir_read_dir,
    // We do not have a real poll story for pseudo directories yet.
    poll: |_, _| Err(SysError::NotYetImplemented),
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};
