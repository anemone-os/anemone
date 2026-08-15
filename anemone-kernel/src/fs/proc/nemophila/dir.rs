//! Dynamic instance directory with open-time enumeration snapshots.

use alloc::boxed::Box;

use crate::{
    fs::{iomux::PollEvent, proc::root::PROC_ROOT_INO},
    nemophila::{InstanceIdentity, snapshots},
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::instance::new_instance_inode;

#[derive(Opaque)]
struct DirectorySnapshot {
    identities: Box<[InstanceIdentity]>,
}

fn directory_snapshot(file: &File) -> &DirectorySnapshot {
    file.prv()
        .cast::<DirectorySnapshot>()
        .expect("/proc/nemophila directory opened without a snapshot")
}

fn lookup(dir: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
    if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(SysError::NotFound);
    }
    let raw = name.parse::<u64>().map_err(|_| SysError::NotFound)?;
    let identity = InstanceIdentity::from_raw(raw).ok_or(SysError::NotFound)?;
    if crate::nemophila::snapshot_instance(identity).is_none() {
        return Err(SysError::NotFound);
    }
    new_instance_inode(dir, identity)
}

fn open(_inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let identities = snapshots()
        .into_iter()
        .map(|snapshot| snapshot.identity)
        .collect::<Vec<_>>()
        .into_boxed_slice();
    Ok(OpenedFile::new(
        &PROC_NEMOPHILA_DIR_FILE_OPS,
        AnyOpaque::new(DirectorySnapshot { identities }),
    ))
}

fn get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    super::readonly_attr(inode, 2, 0)
}

pub(super) static PROC_NEMOPHILA_DIR_INODE_OPS: InodeOps = InodeOps {
    make_node: reject_make_node,
    lookup,
    touch: |_, _, _| Err(SysError::NotSupported),
    mkdir: |_, _, _| Err(SysError::NotSupported),
    symlink: |_, _, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::IsDir),
    unlink: |_, _| Err(SysError::IsDir),
    rmdir: |_, _| Err(SysError::NotSupported),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open,
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::IsDir),
    get_attr,
};

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
    let snapshot = directory_snapshot(file);
    let old_pos = *pos;
    while *pos < snapshot.identities.len() + 2 {
        let result = match *pos {
            0 => push(sink, ".".to_string(), file.inode().ino(), InodeType::Dir)?,
            1 => push(sink, "..".to_string(), PROC_ROOT_INO, InodeType::Dir)?,
            index => push(
                sink,
                snapshot.identities[index - 2].raw().to_string(),
                Ino::INVALID,
                InodeType::Regular,
            )?,
        };
        match result {
            SinkResult::Accepted => *pos += 1,
            SinkResult::Stop => break,
        }
    }
    Ok(if *pos == old_pos {
        ReadDirResult::Eof
    } else {
        ReadDirResult::Progressed
    })
}

static PROC_NEMOPHILA_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::IsDir),
    write: |_, _, _, _| Err(SysError::IsDir),
    read_at: |_, _, _, _| Err(SysError::IsDir),
    write_at: |_, _, _, _| Err(SysError::IsDir),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: seek_dir_rewind,
    read_dir,
    poll: |_, request| Ok(request.ready_or_unsupported(PollEvent::READABLE & request.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};
