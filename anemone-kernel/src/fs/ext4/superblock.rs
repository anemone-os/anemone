use crate::{
    fs::{
        ext4::{
            ext4_ino, ext4_sb,
            file::{Ext4Reg, Ext4RegMapping},
            inode::{
                EXT4_DEV_INODE_OPS, EXT4_DIR_INODE_OPS, EXT4_REG_INODE_OPS, EXT4_SYMLINK_INODE_OPS,
            },
            map_ext4_error, map_lwext4_inode_type,
        },
        inode::{Inode, InodeMeta},
        superblock::{FsMagic, FsStat, SuperBlockOps},
    },
    prelude::{vmo::VmObject, *},
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use anemone_abi::fs::linux::stat::EXT4_SUPER_MAGIC;

fn ext4_inode_ops(ty: InodeType) -> &'static InodeOps {
    match ty {
        InodeType::Dir => &EXT4_DIR_INODE_OPS,
        InodeType::Regular => &EXT4_REG_INODE_OPS,
        InodeType::Block | InodeType::Char => &EXT4_DEV_INODE_OPS,
        InodeType::Symlink => &EXT4_SYMLINK_INODE_OPS,
        InodeType::Fifo => unimplemented!("ext4 fifo inode ops"),
    }
}

fn ext4_load_inode(sb: &Arc<SuperBlock>, ino: Ino) -> Result<Arc<Inode>, SysError> {
    let attr = ext4_sb(sb).read_tx(|| {
        ext4_sb(sb).with_fs(|fs| {
            let mut attr = lwext4_rust::FileAttr::default();
            fs.get_attr(ino.get() as u32, &mut attr)
                .map_err(map_ext4_error)?;

            Ok(attr)
        })
    })?;

    let ty = map_lwext4_inode_type(attr.node_type)?;

    let (prv, mapping) = if ty == InodeType::Regular {
        let prv = Ext4Reg::new(sb.clone(), ino, attr.size as usize);
        let mapping = Ext4RegMapping::new(prv.state().clone());
        (
            AnyOpaque::new(prv),
            Some(Arc::new(mapping) as Arc<dyn VmObject>),
        )
    } else {
        (NilOpaque::new(), None)
    };

    let mut inode = Inode::new(ext4_ino(attr.ino)?, ty, ext4_inode_ops(ty), sb.clone(), prv);

    inode.set_mapping(mapping);

    inode.set_meta(&InodeMeta {
        nlink: attr.nlink,
        perm: InodePerm::from_bits_truncate(attr.mode as u16),
        uid: Uid::new(attr.uid),
        gid: Gid::new(attr.gid),
        size: attr.size,
        atime: attr.atime,
        mtime: attr.mtime,
        ctime: attr.ctime,
    });

    kdebugln!("ext4: loaded inode {:?} into icache", ino);
    Ok(Arc::new(inode))
}

fn ext4_sync_inode_inner(inode: &Arc<Inode>) -> Result<(), SysError> {
    // `indexed` cannot be used as a deletion test here: VFS clears it before
    // invoking `evict_inode`, so a still-live inode being evicted also arrives
    // here as unindexed. The real terminal state is `nlink == 0`: final
    // unlink/rmdir has already committed that deletion to ext4, and lwext4 may
    // have freed the on-disk inode immediately, so reopening it by ino for a
    // later metadata sync would be invalid.
    if inode.nlink() == 0 {
        return Ok(());
    }

    let sb = inode.sb();
    let ino = inode.ino();
    let meta = inode.meta_snapshot();

    if inode.ty() == InodeType::Regular {
        inode
            .prv()
            .cast::<Ext4Reg>()
            .expect("regular inode must have Ext4Reg as its private data")
            .sync_all()?;
    }

    ext4_sb(&sb).write_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            fs.with_inode_ref(ino.get() as u32, |inode_ref| {
                if inode_ref.size() != meta.size {
                    inode_ref.set_len(meta.size)?;
                }
                // TODO: implement lwext4's Hal and use update_[x]time directly
                inode_ref.set_atime(&meta.atime);
                inode_ref.set_mtime(&meta.mtime);
                inode_ref.set_ctime(&meta.ctime);
                inode_ref.set_mode(InodeMode::new(inode.ty(), meta.perm).to_linux_mode());
                // TODO: persist uid/gid once lwext4_rust exposes explicit
                // setters for inode owner fields.
                Ok(())
            })
            .map_err(map_ext4_error)?;
            fs.flush().map_err(map_ext4_error)
        })
    })
}

fn ext4_evict_inode(inode: Arc<Inode>) -> Result<(), SysError> {
    ext4_sync_inode_inner(&inode)
}

fn ext4_sync_inode(inode: &InodeRef) -> Result<(), SysError> {
    ext4_sync_inode_inner(&inode.inode())
}

fn ext4_stat(sb: &SuperBlock) -> Result<FsStat, SysError> {
    let stat =
        ext4_sb(sb).read_tx(|| ext4_sb(sb).with_fs(|fs| fs.stat().map_err(map_ext4_error)))?;

    let blocks_free = stat.free_blocks_count.min(stat.blocks_count);
    let files = stat.inodes_count as u64;
    let files_free = (stat.free_inodes_count as u64).min(files);

    Ok(FsStat {
        magic: FsMagic::new(EXT4_SUPER_MAGIC),
        block_size: stat.block_size as u64,
        fragment_size: stat.block_size as u64,
        blocks: stat.blocks_count,
        blocks_free,
        blocks_available: blocks_free,
        files,
        files_free,
        name_max: 255,
    })
}

pub(super) static EXT4_SB_OPS: SuperBlockOps = SuperBlockOps {
    load_inode: ext4_load_inode,
    evict_inode: ext4_evict_inode,
    sync_inode: ext4_sync_inode,
    stat: ext4_stat,
};
