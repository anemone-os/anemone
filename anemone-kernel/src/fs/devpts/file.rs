use crate::prelude::*;

use super::{DevptsInode, devpts_sb};

const DOT_CURSOR: usize = 0;
const DOTDOT_CURSOR: usize = 1;
const SLOT_CURSOR_BASE: usize = 2;

fn push(
    sink: &mut dyn DirSink,
    name: String,
    ino: Ino,
    ty: InodeType,
) -> Result<SinkResult, SysError> {
    sink.push(DirEntry { name, ino, ty })
}

fn read_dir(
    file: &File,
    pos: &mut usize,
    sink: &mut dyn DirSink,
) -> Result<ReadDirResult, SysError> {
    let private = file
        .inode()
        .inode()
        .prv()
        .cast::<DevptsInode>()
        .expect("devpts directory private state mismatch");
    if !matches!(private, DevptsInode::Root) {
        return Err(SysError::NotDir);
    }
    let root_ino = file.inode().ino();
    let mut pushed = false;
    loop {
        let result = match *pos {
            DOT_CURSOR => Some(push(sink, ".".to_string(), root_ino, InodeType::Dir)?),
            DOTDOT_CURSOR => Some(push(sink, "..".to_string(), root_ino, InodeType::Dir)?),
            _ => None,
        };
        let Some(result) = result else { break };
        match result {
            SinkResult::Accepted => {
                pushed = true;
                *pos += 1;
            },
            SinkResult::Stop => {
                return Ok(if pushed {
                    ReadDirResult::Progressed
                } else {
                    ReadDirResult::Eof
                });
            },
        }
    }

    let entries = devpts_sb(file.path().mount().sb()).core.live_entries();
    while *pos - SLOT_CURSOR_BASE < PTY_SYSTEM_CAPACITY {
        let index = *pos - SLOT_CURSOR_BASE;
        *pos += 1;
        let Some((_, ino)) = entries.iter().find(|(slot, _)| *slot == index) else {
            continue;
        };
        match push(sink, index.to_string(), *ino, InodeType::Char)? {
            SinkResult::Accepted => pushed = true,
            SinkResult::Stop => {
                *pos -= 1;
                return Ok(if pushed {
                    ReadDirResult::Progressed
                } else {
                    ReadDirResult::Eof
                });
            },
        }
    }
    Ok(if pushed {
        ReadDirResult::Progressed
    } else {
        ReadDirResult::Eof
    })
}

pub(super) static DEVPTS_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::IsDir),
    write: |_, _, _, _| Err(SysError::IsDir),
    read_at: |_, _, _, _| Err(SysError::IsDir),
    write_at: |_, _, _, _| Err(SysError::IsDir),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: seek_dir_rewind,
    read_dir,
    poll: |_, _| Err(SysError::NotYetImplemented),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};
