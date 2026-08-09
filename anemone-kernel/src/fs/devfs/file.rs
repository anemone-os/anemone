use crate::prelude::*;

use super::inode::devfs_inode_node;

const DEVFS_DOT_CURSOR: usize = 0;
const DEVFS_DOTDOT_CURSOR: usize = 1;
const DEVFS_ENTRY_CURSOR_BASE: usize = 2;

fn push_dir_entry(
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

fn devfs_dir_read_dir(
    file: &File,
    pos: &mut usize,
    sink: &mut dyn DirSink,
) -> Result<ReadDirResult, SysError> {
    let mut pushed_any = false;
    let node = devfs_inode_node(file.inode());
    assert!(
        node.is_directory(),
        "non-directory devfs inode opened with directory file ops"
    );

    loop {
        match *pos {
            DEVFS_DOT_CURSOR => match push_dir_entry(sink, ".", node.ino(), InodeType::Dir)? {
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
                match push_dir_entry(sink, "..", node.parent_ino(), InodeType::Dir)? {
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

    // Devfs publication is append-only, so a plain index remains stable while
    // new entries are appended after the current cursor.
    while let Some(child) = node.child_at(*pos - DEVFS_ENTRY_CURSOR_BASE) {
        match push_dir_entry(sink, child.name(), child.ino(), child.attr().ty)? {
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

pub(super) static DEVFS_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::IsDir),
    write: |_, _, _, _| Err(SysError::IsDir),
    read_at: |_, _, _, _| Err(SysError::IsDir),
    write_at: |_, _, _, _| Err(SysError::IsDir),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: seek_dir_rewind,
    read_dir: devfs_dir_read_dir,
    // We do not have a real poll story for pseudo directories yet.
    poll: |_, _| Err(SysError::NotYetImplemented),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};
