use crate::{
    fs::{inode::Inode, superblock::SuperBlockOps},
    prelude::*,
};

fn proc_load_inode(sn: &Arc<SuperBlock>, ino: Ino) -> Result<Arc<Inode>, SysError> {
    todo!()
}

fn proc_evict_inode(inode: Arc<Inode>) -> Result<(), SysError> {
    todo!()
}

fn proc_sync_inode(inode: &InodeRef) -> Result<(), SysError> {
    todo!()
}

pub(super) static PROC_SB_OPS: SuperBlockOps = SuperBlockOps {
    load_inode: proc_load_inode,
    evict_inode: proc_evict_inode,
    sync_inode: proc_sync_inode,
};
