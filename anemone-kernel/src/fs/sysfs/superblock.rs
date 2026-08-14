use crate::{
    fs::{
        inode::Inode,
        superblock::{FsMagic, FsStat, SuperBlockOps},
    },
    prelude::*,
};

const SYSFS_MAGIC: u64 = 0x6265_6572;

fn load_inode(_sb: &Arc<SuperBlock>, _ino: Ino) -> Result<Arc<Inode>, SysError> {
    unreachable!("static sysfs never reloads an inode")
}

fn evict_inode(_inode: Arc<Inode>) -> Result<(), SysError> {
    Ok(())
}

fn sync_inode(_inode: &InodeRef) -> Result<(), SysError> {
    Ok(())
}

fn stat(_sb: &SuperBlock) -> Result<FsStat, SysError> {
    Ok(FsStat::pseudo(FsMagic::new(SYSFS_MAGIC)))
}

pub(super) static SYSFS_SB_OPS: SuperBlockOps = SuperBlockOps {
    load_inode,
    evict_inode,
    sync_inode,
    stat,
};
