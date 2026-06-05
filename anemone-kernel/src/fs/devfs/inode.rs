use crate::{
    fs::{
        devfs::{DEVFS_ROOT_INO, DevfsNode, DevfsNodeAttr, DevfsNodeKind, published_node_by_name},
        inode::{Inode, RenameFlags},
    },
    prelude::*,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::file::{DEVFS_DIR_FILE_OPS, DEVFS_ROOT_FILE_OPS};

#[derive(Opaque)]
struct DevfsInode {
    node: Arc<DevfsNode>,
}

fn devfs_inode_node(inode: &InodeRef) -> &Arc<DevfsNode> {
    &inode
        .inode()
        .prv()
        .cast::<DevfsInode>()
        .expect("devfs leaf inode must carry DevfsInode private data")
        .node
}

fn make_inode_stat(inode: &InodeRef, attr: DevfsNodeAttr, size: u64) -> InodeStat {
    let meta = inode.inode().meta_snapshot();

    InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: InodeMode::new(attr.ty, meta.perm),
        nlink: meta.nlink,
        uid: meta.uid,
        gid: meta.gid,
        rdev: attr.rdev,
        size,
        atime: meta.atime,
        mtime: meta.mtime,
        ctime: meta.ctime,
    }
}

fn devfs_dir_open(_inode: &InodeRef) -> Result<OpenedFile, SysError> {
    Ok(OpenedFile {
        file_ops: &DEVFS_DIR_FILE_OPS,
        prv: NilOpaque::new(),
    })
}

fn devfs_dir_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    Ok(make_inode_stat(
        inode,
        DevfsNodeAttr {
            ty: InodeType::Dir,
            perm: inode.perm(),
            rdev: DeviceId::None,
        },
        0,
    ))
}

pub(super) fn devfs_new_root_inode(sb: Arc<SuperBlock>) -> Arc<Inode> {
    let inode = Arc::new(Inode::new(
        DEVFS_ROOT_INO,
        InodeType::Dir,
        &DEVFS_ROOT_INODE_OPS,
        sb,
        NilOpaque::new(),
    ));

    inode.set_nlink(2);
    // Directories need execute permission for traversal, so `/dev` itself
    // remains searchable even though device leaves below it are non-executable.
    inode.set_perm(InodePerm::all_rwx());
    inode.set_size(0);
    inode.set_times(
        Instant::ZERO.to_duration(),
        Instant::ZERO.to_duration(),
        Instant::ZERO.to_duration(),
    );

    inode
}

pub(super) fn devfs_new_node_inode(sb: Arc<SuperBlock>, node: Arc<DevfsNode>) -> Arc<Inode> {
    let attr = node.attr;
    let inode = Arc::new(Inode::new(
        node.ino,
        attr.ty,
        &DEVFS_NODE_INODE_OPS,
        sb,
        AnyOpaque::new(DevfsInode { node }),
    ));

    inode.set_nlink(match attr.ty {
        InodeType::Dir => 2,
        _ => 1,
    });
    inode.set_perm(attr.perm);
    inode.set_size(0);
    inode.set_times(
        Instant::ZERO.to_duration(),
        Instant::ZERO.to_duration(),
        Instant::ZERO.to_duration(),
    );

    inode
}

fn devfs_root_lookup(dir: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
    if matches!(name, "." | "..") {
        return Ok(dir.sb().root_inode());
    }

    let node = published_node_by_name(name).ok_or(SysError::NotFound)?;

    // Leaf inodes are seeded at publish time, so lookup should only resolve an
    // existing icache entry here.
    Ok(dir
        .sb()
        .try_iget(node.ino)
        .expect("published devfs inode missing from icache"))
}

fn devfs_root_open(_inode: &InodeRef) -> Result<OpenedFile, SysError> {
    Ok(OpenedFile {
        file_ops: &DEVFS_ROOT_FILE_OPS,
        prv: NilOpaque::new(),
    })
}

fn devfs_node_lookup(inode: &InodeRef, _name: &str) -> Result<InodeRef, SysError> {
    match devfs_inode_node(inode).kind {
        DevfsNodeKind::Dir => Err(SysError::NotFound),
        DevfsNodeKind::Leaf => Err(SysError::NotDir),
    }
}

fn devfs_node_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let node = devfs_inode_node(inode);
    match node.kind {
        DevfsNodeKind::Dir => devfs_dir_open(inode),
        DevfsNodeKind::Leaf => node
            .ops
            .as_ref()
            .expect("devfs leaf node must carry DevfsNodeOps")
            .open(inode),
    }
}

fn devfs_node_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let node = devfs_inode_node(inode);
    match node.kind {
        DevfsNodeKind::Dir => devfs_dir_get_attr(inode),
        DevfsNodeKind::Leaf => node
            .ops
            .as_ref()
            .expect("devfs leaf node must carry DevfsNodeOps")
            .get_attr(inode, node.attr),
    }
}

fn devfs_node_touch(inode: &InodeRef, _name: &str, _perm: InodePerm) -> Result<InodeRef, SysError> {
    match devfs_inode_node(inode).kind {
        DevfsNodeKind::Dir => Err(SysError::NotSupported),
        DevfsNodeKind::Leaf => Err(SysError::NotDir),
    }
}

fn devfs_node_mkdir(inode: &InodeRef, _name: &str, _perm: InodePerm) -> Result<InodeRef, SysError> {
    match devfs_inode_node(inode).kind {
        DevfsNodeKind::Dir => Err(SysError::NotSupported),
        DevfsNodeKind::Leaf => Err(SysError::NotDir),
    }
}

fn devfs_node_symlink(inode: &InodeRef, _name: &str, _target: &Path) -> Result<InodeRef, SysError> {
    match devfs_inode_node(inode).kind {
        DevfsNodeKind::Dir => Err(SysError::NotSupported),
        DevfsNodeKind::Leaf => Err(SysError::NotDir),
    }
}

fn devfs_node_link(inode: &InodeRef, _name: &str, _target: &InodeRef) -> Result<(), SysError> {
    match devfs_inode_node(inode).kind {
        DevfsNodeKind::Dir => Err(SysError::IsDir),
        DevfsNodeKind::Leaf => Err(SysError::NotDir),
    }
}

fn devfs_node_unlink(inode: &InodeRef, _name: &str) -> Result<(), SysError> {
    match devfs_inode_node(inode).kind {
        DevfsNodeKind::Dir => Err(SysError::IsDir),
        DevfsNodeKind::Leaf => Err(SysError::NotDir),
    }
}

fn devfs_node_rmdir(inode: &InodeRef, _name: &str) -> Result<(), SysError> {
    match devfs_inode_node(inode).kind {
        DevfsNodeKind::Dir => Err(SysError::NotSupported),
        DevfsNodeKind::Leaf => Err(SysError::NotDir),
    }
}

fn devfs_node_rename(
    inode: &InodeRef,
    _old_name: &str,
    _new_dir: &InodeRef,
    _new_name: &str,
    _flags: RenameFlags,
) -> Result<(), SysError> {
    match devfs_inode_node(inode).kind {
        DevfsNodeKind::Dir => Err(SysError::NotSupported),
        DevfsNodeKind::Leaf => Err(SysError::NotSupported),
    }
}

pub(super) static DEVFS_ROOT_INODE_OPS: InodeOps = InodeOps {
    lookup: devfs_root_lookup,
    touch: |_, _, _| Err(SysError::NotSupported),
    mkdir: |_, _, _| Err(SysError::NotSupported),
    symlink: |_, _, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::IsDir),
    unlink: |_, _| Err(SysError::IsDir),
    rmdir: |_, _| Err(SysError::NotSupported),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: devfs_root_open,
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: devfs_dir_get_attr,
};

pub(super) static DEVFS_NODE_INODE_OPS: InodeOps = InodeOps {
    lookup: devfs_node_lookup,
    touch: devfs_node_touch,
    mkdir: devfs_node_mkdir,
    symlink: devfs_node_symlink,
    link: devfs_node_link,
    unlink: devfs_node_unlink,
    rmdir: devfs_node_rmdir,
    rename: devfs_node_rename,
    open: devfs_node_open,
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: devfs_node_get_attr,
};
