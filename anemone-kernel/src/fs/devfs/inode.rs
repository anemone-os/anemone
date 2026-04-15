use crate::{
    device::{block::get_block_dev, char::get_char_dev},
    fs::{
        devfs::{DevfsNode, devfs_ino_for, devfs_inode_data, devfs_lookup_name, devfs_root_ino},
        inode::Inode,
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::file::{DEVFS_BLOCK_FILE_OPS, DEVFS_CHAR_FILE_OPS, DEVFS_DIR_FILE_OPS, DevfsFile};

#[derive(Debug, Clone, Copy, Opaque)]
pub(super) struct DevfsInode {
    node: DevfsNode,
}

impl DevfsInode {
    fn new(node: DevfsNode) -> Self {
        Self { node }
    }

    fn node(self) -> DevfsNode {
        self.node
    }
}

pub(super) fn devfs_new_inode(
    sb: Arc<SuperBlock>,
    node: DevfsNode,
) -> Result<Arc<Inode>, SysError> {
    let inode = Arc::new(Inode::new(
        devfs_ino_for(node),
        match node {
            DevfsNode::Root => InodeType::Dir,
            DevfsNode::Char(_) => InodeType::Char,
            DevfsNode::Block(_) => InodeType::Block,
        },
        match node {
            DevfsNode::Root => &DEVFS_ROOT_INODE_OPS,
            DevfsNode::Char(_) | DevfsNode::Block(_) => &DEVFS_DEV_INODE_OPS,
        },
        sb,
        AnyOpaque::new(DevfsInode::new(node)),
    ));

    match node {
        DevfsNode::Root => {
            inode.set_nlink(2);
            inode.set_perm(InodePerm::all_rwx());
            inode.set_size(0);
        },
        DevfsNode::Char(_) => {
            inode.set_nlink(1);
            inode.set_perm(InodePerm::all_rwx());
            inode.set_size(0);
        },
        DevfsNode::Block(devnum) => {
            let dev = get_block_dev(devnum).ok_or(SysError::NotFound)?;
            inode.set_nlink(1);
            inode.set_perm(InodePerm::all_rwx());
            inode.set_size((dev.block_size().bytes() * dev.total_blocks()) as u64);
        },
    }

    Ok(inode)
}

fn devfs_lookup(dir: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
    match devfs_inode_data(dir).node() {
        DevfsNode::Root => {
            let node = devfs_lookup_name(name)?;
            dir.sb().iget(devfs_ino_for(node))
        },
        _ => Err(SysError::NotDir),
    }
}

fn devfs_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let node = devfs_inode_data(inode).node();

    Ok(OpenedFile {
        file_ops: match node {
            DevfsNode::Root => &DEVFS_DIR_FILE_OPS,
            DevfsNode::Char(_) => &DEVFS_CHAR_FILE_OPS,
            DevfsNode::Block(_) => &DEVFS_BLOCK_FILE_OPS,
        },
        prv: AnyOpaque::new(DevfsFile::new(node)),
    })
}

fn devfs_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let node = devfs_inode_data(inode).node();
    let meta = inode.inode().meta_snapshot();

    let (mode, nlink, rdev, size) = match node {
        DevfsNode::Root => (
            InodeMode::new(InodeType::Dir, meta.perm),
            2,
            DeviceId::None,
            0,
        ),
        DevfsNode::Char(devnum) => {
            get_char_dev(devnum).ok_or(SysError::NotFound)?;
            (
                InodeMode::new(InodeType::Char, meta.perm),
                1,
                DeviceId::Char(devnum),
                0,
            )
        },
        DevfsNode::Block(devnum) => {
            let dev = get_block_dev(devnum).ok_or(SysError::NotFound)?;
            (
                InodeMode::new(InodeType::Block, meta.perm),
                1,
                DeviceId::Block(devnum),
                (dev.block_size().bytes() * dev.total_blocks()) as u64,
            )
        },
    };

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: match node {
            DevfsNode::Root => devfs_root_ino(),
            _ => inode.ino(),
        },
        mode,
        nlink,
        uid: 0,
        gid: 0,
        rdev,
        size,
        atime: meta.atime,
        mtime: meta.mtime,
        ctime: meta.ctime,
    })
}

pub(super) static DEVFS_ROOT_INODE_OPS: InodeOps = InodeOps {
    lookup: devfs_lookup,
    touch: |_, _, _| Err(SysError::NotSupported),
    mkdir: |_, _, _| Err(SysError::NotSupported),
    symlink: |_, _, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::NotSupported),
    unlink: |_, _| Err(SysError::NotSupported),
    rmdir: |_, _| Err(SysError::NotSupported),
    open: devfs_open,
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: devfs_get_attr,
};

pub(super) static DEVFS_DEV_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    open: devfs_open,
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: devfs_get_attr,
};
