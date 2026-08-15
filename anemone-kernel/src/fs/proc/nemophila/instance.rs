//! Per-instance open-time immutable diagnostic files.

use alloc::{boxed::Box, format};

use crate::{
    fs::{
        inode::Inode,
        iomux::PollEvent,
        proc::{read_snapshot_at, superblock::alloc_ino},
    },
    nemophila::{
        InstanceIdentity, InstanceOrigin, InstanceSnapshot, LifecycleSnapshot, snapshot_instance,
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::readonly_attr;

#[derive(Opaque)]
struct InstanceInodePrivate {
    identity: InstanceIdentity,
}

#[derive(Opaque)]
struct InstanceFileSnapshot {
    text: Box<str>,
}

fn inode_identity(inode: &InodeRef) -> InstanceIdentity {
    inode
        .inode()
        .prv()
        .cast::<InstanceInodePrivate>()
        .expect("/proc/nemophila instance inode lacks identity")
        .identity
}

fn file_snapshot(file: &File) -> &InstanceFileSnapshot {
    file.prv()
        .cast::<InstanceFileSnapshot>()
        .expect("/proc/nemophila instance opened without text snapshot")
}

fn render(snapshot: InstanceSnapshot) -> Box<str> {
    let (source, artifact) = match snapshot.origin {
        InstanceOrigin::Embedded(identity) => ("embedded", identity),
        InstanceOrigin::Supplied => ("supplied", "-"),
    };
    let lifecycle = match snapshot.lifecycle {
        LifecycleSnapshot::Live => "live",
        LifecycleSnapshot::Poisoned => "poisoned",
    };
    format!(
        "instance: {}\nsource: {}\nartifact: {}\nlifecycle: {}\nin_flight: {}\n",
        snapshot.identity.raw(),
        source,
        artifact,
        lifecycle,
        snapshot.in_flight
    )
    .into_boxed_str()
}

fn open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    // Rechecking membership here prevents a cached positive dentry from
    // reopening a retired instance. Once open succeeds, the copied text is
    // intentionally independent and remains readable after retirement.
    let snapshot = snapshot_instance(inode_identity(inode)).ok_or(SysError::NotFound)?;
    Ok(OpenedFile::new(
        &PROC_NEMOPHILA_INSTANCE_FILE_OPS,
        AnyOpaque::new(InstanceFileSnapshot {
            text: render(snapshot),
        }),
    ))
}

fn get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let snapshot = snapshot_instance(inode_identity(inode)).ok_or(SysError::NotFound)?;
    readonly_attr(inode, 1, render(snapshot).len() as u64)
}

static PROC_NEMOPHILA_INSTANCE_INODE_OPS: InodeOps = InodeOps {
    make_node: reject_make_node,
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open,
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr,
};

pub(super) fn new_instance_inode(
    dir: &InodeRef,
    identity: InstanceIdentity,
) -> Result<InodeRef, SysError> {
    let inode = Inode::new(
        alloc_ino(),
        InodeType::Regular,
        &PROC_NEMOPHILA_INSTANCE_INODE_OPS,
        dir.sb(),
        AnyOpaque::new(InstanceInodePrivate { identity }),
    );
    inode.set_meta(&InodeMeta {
        nlink: 1,
        size: 0,
        perm: InodePerm::all_r(),
        uid: Uid::ROOT,
        gid: Gid::ROOT,
        atime: Duration::ZERO,
        mtime: Duration::ZERO,
        ctime: Duration::ZERO,
    });
    // Generic procfs/VFS does not yet provide dynamic inode interning and
    // retirement. Keep that known gap explicit instead of adding a second,
    // Nemophila-private namespace registry; see the active register issue.
    Ok(InodeRef::new(Arc::new(inode)))
}

fn read(file: &File, pos: &mut usize, buf: &mut [u8], _ctx: FileIoCtx) -> Result<usize, SysError> {
    let read = read_snapshot_at(*pos, buf, file_snapshot(file).text.as_bytes())?;
    *pos += read;
    Ok(read)
}

fn read_at(file: &File, pos: usize, buf: &mut [u8], _ctx: FileIoCtx) -> Result<usize, SysError> {
    read_snapshot_at(pos, buf, file_snapshot(file).text.as_bytes())
}

fn seek(file: &File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError> {
    seek_with_bounded_size(file, pos, from, file_snapshot(file).text.len())
}

static PROC_NEMOPHILA_INSTANCE_FILE_OPS: FileOps = FileOps {
    read,
    write: |_, _, _, _| Err(SysError::NotSupported),
    read_at,
    write_at: |_, _, _, _| Err(SysError::NotSupported),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, request| Ok(request.ready_or_unsupported(PollEvent::READABLE & request.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};
