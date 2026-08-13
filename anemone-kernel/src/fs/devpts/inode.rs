use crate::{
    device::{
        devnum::{DeviceNumber, MajorNum, MinorNum},
        tty::PreparedPtySlaveDescription,
    },
    fs::inode::{Inode, RenameFlags},
    prelude::*,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::{
    DEVPTS_ROOT_INO, DEVPTS_SLAVE_MAJOR, DevptsBinding, devpts_sb, file::DEVPTS_DIR_FILE_OPS,
};

#[derive(Opaque)]
pub(super) enum DevptsInode {
    Root,
    Slave {
        episode: super::EpisodeId,
        binding: Weak<DevptsBinding>,
    },
}

fn private(inode: &InodeRef) -> &DevptsInode {
    inode
        .inode()
        .prv()
        .cast::<DevptsInode>()
        .expect("devpts inode private state mismatch")
}

pub(super) fn new_root_inode(sb: Arc<SuperBlock>) -> Result<Arc<Inode>, SysError> {
    let inode = Arc::try_new(Inode::new(
        DEVPTS_ROOT_INO,
        InodeType::Dir,
        &DEVPTS_INODE_OPS,
        sb,
        AnyOpaque::new(DevptsInode::Root),
    ))
    .map_err(|_| SysError::OutOfMemory)?;
    inode.set_meta(&InodeMeta {
        nlink: 2,
        size: 0,
        perm: InodePerm::all_rwx(),
        uid: Uid::ROOT,
        gid: Gid::ROOT,
        atime: Duration::ZERO,
        mtime: Duration::ZERO,
        ctime: Duration::ZERO,
    });
    Ok(inode)
}

pub(super) fn new_slave_inode(
    sb: Arc<SuperBlock>,
    binding: Weak<DevptsBinding>,
    uid: Uid,
    gid: Gid,
) -> Result<Arc<Inode>, SysError> {
    let owner = binding.upgrade().ok_or(SysError::IO)?;
    let episode = owner.episode();
    drop(owner);
    let inode = Arc::try_new(Inode::new(
        episode.ino,
        InodeType::Char,
        &DEVPTS_INODE_OPS,
        sb,
        AnyOpaque::new(DevptsInode::Slave { episode, binding }),
    ))
    .map_err(|_| SysError::OutOfMemory)?;
    inode.set_meta(&InodeMeta {
        nlink: 1,
        size: 0,
        perm: InodePerm::IRUSR | InodePerm::IWUSR,
        uid,
        gid,
        atime: Duration::ZERO,
        mtime: Duration::ZERO,
        ctime: Duration::ZERO,
    });
    Ok(inode)
}

fn root_binding(inode: &InodeRef, name: &str) -> Result<Arc<DevptsBinding>, SysError> {
    if !matches!(private(inode), DevptsInode::Root) {
        return Err(SysError::NotDir);
    }
    let index = name.parse::<usize>().map_err(|_| SysError::NotFound)?;
    if index.to_string() != name {
        return Err(SysError::NotFound);
    }
    devpts_sb(&inode.sb())
        .core
        .binding(index)
        .ok_or(SysError::NotFound)
}

fn devpts_lookup(inode: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
    match name {
        "." | ".." if matches!(private(inode), DevptsInode::Root) => return Ok(inode.clone()),
        _ => {},
    }
    let binding = root_binding(inode, name)?;
    binding.active_inode().ok_or(SysError::NotFound)
}

fn prepare_slave_description(
    state: AnyOpaque,
    request: FileOpenRequest,
    description_ops: crate::task::files::FileDescOps,
) -> Result<PreparedOpenDescription, SysError> {
    let state = state
        .cast::<SlaveActivation>()
        .expect("devpts slave activation type mismatch");
    let mut prepared = state
        .description
        .lock()
        .take()
        .expect("devpts slave activation consumed more than once");
    let description_ops = prepared.compose_description_ops(description_ops);
    let relation = state
        .pair
        .prepare_implicit_acquire(request.access().can_read(), request.no_ctty());
    Ok(PreparedOpenDescription {
        description_ops,
        commit: OpenDescriptionCommit::new(
            AnyOpaque::new(SlaveCommit {
                description: SpinLock::new(Some(prepared)),
                relation: SpinLock::new(Some(relation)),
            }),
            commit_slave_description,
        ),
    })
}

fn commit_slave_description(
    state: AnyOpaque,
    description: Arc<crate::task::files::FileDesc>,
) -> Result<(), SysError> {
    let state = state
        .cast::<SlaveCommit>()
        .expect("devpts slave commit type mismatch");
    let prepared = state
        .description
        .lock()
        .take()
        .expect("devpts slave commit consumed more than once");
    let relation = state
        .relation
        .lock()
        .take()
        .expect("devpts slave relation effect consumed more than once");
    prepared.commit(description, |_| relation.commit())
}

#[derive(Opaque)]
struct SlaveActivation {
    pair: crate::device::tty::LivePtyPair,
    description: SpinLock<Option<PreparedPtySlaveDescription>>,
}

#[derive(Opaque)]
struct SlaveCommit {
    description: SpinLock<Option<PreparedPtySlaveDescription>>,
    relation: SpinLock<Option<crate::device::tty::PtyImplicitAcquire>>,
}

fn devpts_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    match private(inode) {
        DevptsInode::Root => Ok(OpenedFile::new(&DEVPTS_DIR_FILE_OPS, NilOpaque::new())),
        DevptsInode::Slave { episode, binding } => {
            let binding = binding.upgrade().ok_or(SysError::IO)?;
            if binding.episode() != *episode || !binding.is_active() {
                return Err(SysError::IO);
            }
            let pair = binding.pair();
            let mut prepared = pair.prepare_slave_description()?;
            let opened = prepared.take_opened_file();
            let OpenedFile {
                file_ops,
                mode,
                prv,
                description_activation: _,
            } = opened;
            Ok(OpenedFile::with_description_activation(
                file_ops,
                mode,
                prv,
                OpenDescriptionActivation::new(
                    AnyOpaque::new(SlaveActivation {
                        pair,
                        description: SpinLock::new(Some(prepared)),
                    }),
                    prepare_slave_description,
                ),
            ))
        },
    }
}

fn devpts_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let meta = inode.inode().meta_snapshot();
    let rdev = match private(inode) {
        DevptsInode::Root => DeviceId::None,
        DevptsInode::Slave { episode, .. } => DeviceId::Number(DeviceNumber::new(
            MajorNum::new(DEVPTS_SLAVE_MAJOR),
            MinorNum::new(episode.index),
        )),
    };
    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: InodeMode::new(inode.ty(), meta.perm),
        nlink: meta.nlink,
        uid: meta.uid,
        gid: meta.gid,
        rdev,
        size: meta.size,
        atime: meta.atime,
        mtime: meta.mtime,
        ctime: meta.ctime,
    })
}

fn reject_node(inode: &InodeRef) -> Result<InodeRef, SysError> {
    if matches!(private(inode), DevptsInode::Root) {
        Err(SysError::NotSupported)
    } else {
        Err(SysError::NotDir)
    }
}

pub(super) static DEVPTS_INODE_OPS: InodeOps = InodeOps {
    lookup: devpts_lookup,
    touch: |inode, _, _| reject_node(inode),
    make_node: reject_make_node,
    mkdir: |inode, _, _| reject_node(inode),
    symlink: |inode, _, _| reject_node(inode),
    link: |_, _, _| Err(SysError::NotSupported),
    unlink: |_, _| Err(SysError::NotSupported),
    rmdir: |_, _| Err(SysError::NotSupported),
    rename: |_, _, _, _, _flags: RenameFlags| Err(SysError::NotSupported),
    open: devpts_open,
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: devpts_get_attr,
};
