use crate::{
    fs::{
        devfs::{DevfsNodeAttr, namespace::DevfsNode},
        inode::{Inode, RenameFlags},
    },
    prelude::*,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::file::DEVFS_DIR_FILE_OPS;

#[derive(Opaque)]
struct DevfsInode {
    node: Arc<DevfsNode>,
}

pub(super) fn devfs_inode_node(inode: &InodeRef) -> &Arc<DevfsNode> {
    let node = &inode
        .inode()
        .prv()
        .cast::<DevfsInode>()
        .expect("devfs inode must carry DevfsInode private data")
        .node;
    assert!(
        inode.ino() == node.ino(),
        "devfs inode private node identity mismatch"
    );
    assert!(
        inode.ty() == node.attr().ty,
        "devfs inode private node type mismatch"
    );
    node
}

fn make_inode_stat(inode: &InodeRef, attr: DevfsNodeAttr, nlink: u64, size: u64) -> InodeStat {
    let meta = inode.inode().meta_snapshot();

    InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: InodeMode::new(attr.ty, meta.perm),
        nlink,
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
    Ok(OpenedFile::new(&DEVFS_DIR_FILE_OPS, NilOpaque::new()))
}

fn devfs_dir_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let node = devfs_inode_node(inode);
    let nlink = node
        .directory_nlink(inode)
        .expect("devfs directory inode must carry directory node");
    Ok(make_inode_stat(inode, node.attr(), nlink, 0))
}

pub(super) fn devfs_new_inode(
    sb: Arc<SuperBlock>,
    node: Arc<DevfsNode>,
) -> Result<Arc<Inode>, SysError> {
    let attr = node.attr();
    let inode = Arc::try_new(Inode::new(
        node.ino(),
        attr.ty,
        &DEVFS_INODE_OPS,
        sb,
        AnyOpaque::new(DevfsInode { node }),
    ))
    .map_err(|_| SysError::OutOfMemory)?;

    inode.set_nlink(match attr.ty {
        InodeType::Dir => 2,
        _ => 1,
    });
    inode.set_perm(attr.perm);
    inode.set_size(0);
    inode.set_times(Duration::ZERO, Duration::ZERO, Duration::ZERO);

    Ok(inode)
}

fn devfs_lookup(inode: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
    let node = devfs_inode_node(inode);
    if !node.is_directory() {
        return Err(SysError::NotDir);
    }

    let ino = match name {
        "." => node.ino(),
        ".." => node.parent_ino(),
        _ => node.child_by_name(name).ok_or(SysError::NotFound)?.ino(),
    };

    Ok(inode
        .sb()
        .try_iget(ino)
        .expect("published devfs inode missing from icache"))
}

fn devfs_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let node = devfs_inode_node(inode);
    if node.is_directory() {
        devfs_dir_open(inode)
    } else {
        node.leaf_ops()
            .expect("devfs leaf node must carry DevfsNodeOps")
            .open(inode)
    }
}

fn devfs_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let node = devfs_inode_node(inode);
    if node.is_directory() {
        devfs_dir_get_attr(inode)
    } else {
        node.leaf_ops()
            .expect("devfs leaf node must carry DevfsNodeOps")
            .get_attr(inode, node.attr())
    }
}

fn devfs_node_touch(inode: &InodeRef, _name: &str, _perm: InodePerm) -> Result<InodeRef, SysError> {
    if devfs_inode_node(inode).is_directory() {
        Err(SysError::NotSupported)
    } else {
        Err(SysError::NotDir)
    }
}

fn devfs_node_mkdir(inode: &InodeRef, _name: &str, _perm: InodePerm) -> Result<InodeRef, SysError> {
    if devfs_inode_node(inode).is_directory() {
        Err(SysError::NotSupported)
    } else {
        Err(SysError::NotDir)
    }
}

fn devfs_node_symlink(inode: &InodeRef, _name: &str, _target: &Path) -> Result<InodeRef, SysError> {
    if devfs_inode_node(inode).is_directory() {
        Err(SysError::NotSupported)
    } else {
        Err(SysError::NotDir)
    }
}

fn devfs_node_link(inode: &InodeRef, _name: &str, _target: &InodeRef) -> Result<(), SysError> {
    if devfs_inode_node(inode).is_directory() {
        Err(SysError::IsDir)
    } else {
        Err(SysError::NotDir)
    }
}

fn devfs_node_unlink(inode: &InodeRef, _name: &str) -> Result<(), SysError> {
    if devfs_inode_node(inode).is_directory() {
        Err(SysError::IsDir)
    } else {
        Err(SysError::NotDir)
    }
}

fn devfs_node_rmdir(inode: &InodeRef, _name: &str) -> Result<(), SysError> {
    if devfs_inode_node(inode).is_directory() {
        Err(SysError::NotSupported)
    } else {
        Err(SysError::NotDir)
    }
}

fn devfs_node_rename(
    _inode: &InodeRef,
    _old_name: &str,
    _new_dir: &InodeRef,
    _new_name: &str,
    _flags: RenameFlags,
) -> Result<(), SysError> {
    Err(SysError::NotSupported)
}

pub(super) static DEVFS_INODE_OPS: InodeOps = InodeOps {
    make_node: reject_make_node,
    lookup: devfs_lookup,
    touch: devfs_node_touch,
    mkdir: devfs_node_mkdir,
    symlink: devfs_node_symlink,
    link: devfs_node_link,
    unlink: devfs_node_unlink,
    rmdir: devfs_node_rmdir,
    rename: devfs_node_rename,
    open: devfs_open,
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: devfs_get_attr,
};
