use anemone_abi::errno::ENOENT;
use lwext4_rust::InodeType as LwExt4InodeType;

use crate::{
    fs::ext4::{
        ext4_ino, ext4_sb, ext4_sync_cached_nlink, file::EXT4_SYMLINK_FILE_OPS, map_ext4_error,
        map_lwext4_inode_type, map_vfs_inode_type,
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::file::{EXT4_DIR_FILE_OPS, EXT4_REG_FILE_OPS, Ext4File};

fn ext4_lookup_child(dir: &InodeRef, name: &str) -> Result<(Ino, InodeType), FsError> {
    let sb = dir.sb();
    ext4_sb(&sb).with_fs(|fs| {
        let mut result = fs
            .lookup(dir.ino().get() as u32, name)
            .map_err(map_ext4_error)?;
        Ok((
            ext4_ino(result.entry().ino())?,
            map_lwext4_inode_type(result.entry().inode_type())?,
        ))
    })
}

fn ext4_create_child(dir: &InodeRef, name: &str, mode: InodeMode) -> Result<InodeRef, FsError> {
    let sb = dir.sb();
    let raw_ino = ext4_sb(&sb).write_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            match fs.lookup(dir.ino().get() as u32, name) {
                Ok(_) => return Err(FsError::AlreadyExists),
                Err(err) if err.code == ENOENT as i32 => {},
                Err(err) => return Err(map_ext4_error(err)),
            }

            fs.create(
                dir.ino().get() as u32,
                name,
                map_vfs_inode_type(mode.ty())?,
                mode.perm().bits() as u32,
            )
            .map_err(map_ext4_error)
        })
    })?;
    let ino = ext4_ino(raw_ino).expect("internal error: lwext4 returned invalid inode number");

    if mode.ty() == InodeType::Dir {
        dir.inode().inc_nlink();
    }
    Ok(sb.iget(ino)?)
}

fn ext4_open(inode: &InodeRef) -> Result<OpenedFile, FsError> {
    let file_ops = match inode.ty() {
        InodeType::Dir => &EXT4_DIR_FILE_OPS,
        InodeType::Regular => &EXT4_REG_FILE_OPS,
        InodeType::Symlink => &EXT4_SYMLINK_FILE_OPS,
        InodeType::Fifo => unimplemented!("ext4 fifo file"),
        InodeType::Dev => unimplemented!("ext4 dev file"),
    };

    Ok(OpenedFile {
        file_ops,
        prv: AnyOpaque::new(Ext4File::new()),
    })
}

#[inline]
fn ext4_mode_from_attr(node_type: LwExt4InodeType, raw_mode: u32) -> Result<InodeMode, FsError> {
    let ty = map_lwext4_inode_type(node_type)?;
    let perm = InodePerm::from_bits_truncate(raw_mode as u16);
    Ok(InodeMode::new(ty, perm))
}

#[inline]
fn ext4_fs_dev(sb: &SuperBlock) -> DeviceId {
    match sb.backing() {
        MountSource::Block(dev) => DeviceId::Block(dev.devnum()),
        _ => unreachable!(),
    }
}

fn ext4_get_attr(inode: &InodeRef) -> Result<InodeStat, FsError> {
    let sb = inode.sb();
    let attr = ext4_sb(&sb).read_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            let mut attr = lwext4_rust::FileAttr::default();
            fs.get_attr(inode.ino().get() as u32, &mut attr)
                .map_err(map_ext4_error)?;
            Ok(attr)
        })
    })?;

    // some metadata may haven't been synced to disk yet, so we need to look at the
    // in-memory inode state first to get the most up-to-date metadata.

    Ok(InodeStat {
        fs_dev: ext4_fs_dev(&sb),
        ino: inode.ino(),
        mode: InodeMode::new(inode.ty(), inode.perm()),
        nlink: attr.nlink,
        uid: attr.uid,
        gid: attr.gid,
        rdev: DeviceId::None,
        size: attr.size,
        atime: attr.atime,
        mtime: attr.mtime,
        ctime: attr.ctime,
    })
}

fn ext4_lookup(dir: &InodeRef, name: &str) -> Result<InodeRef, FsError> {
    let sb = dir.sb();
    let (ino, _ty) = ext4_sb(&sb).read_tx(|| ext4_lookup_child(dir, name))?;
    dir.sb().iget(ino)
}

fn ext4_create(dir: &InodeRef, name: &str, mode: InodeMode) -> Result<InodeRef, FsError> {
    match mode.ty() {
        InodeType::Regular | InodeType::Dir => ext4_create_child(dir, name, mode),
        InodeType::Symlink => Err(FsError::NotSupported),
        InodeType::Dev | InodeType::Fifo => Err(FsError::NotSupported),
    }
}

fn ext4_symlink(dir: &InodeRef, name: &str, target: &Path) -> Result<InodeRef, FsError> {
    let sb = dir.sb();
    let raw_ino = ext4_sb(&sb).write_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            match fs.lookup(dir.ino().get() as u32, name) {
                Ok(_) => return Err(FsError::AlreadyExists),
                Err(err) if err.code == ENOENT as i32 => {},
                Err(err) => return Err(map_ext4_error(err)),
            }

            let ino = fs
                .create(
                    dir.ino().get() as u32,
                    name,
                    LwExt4InodeType::Symlink,
                    0o777, // permissions of symlink are mostly ignored
                )
                .map_err(map_ext4_error)?;

            fs.set_symlink(ino, target.as_bytes())
                .map_err(map_ext4_error)?;

            Ok(ino)
        })
    })?;

    let ino = ext4_ino(raw_ino).expect("internal error: lwext4 returned invalid inode number");

    sb.iget(ino)
}

fn ext4_link(dir: &InodeRef, name: &str, target: &InodeRef) -> Result<(), FsError> {
    if target.ty() == InodeType::Dir {
        return Err(FsError::IsDir);
    }

    let sb = dir.sb();
    if !Arc::ptr_eq(&sb, &target.sb()) {
        return Err(FsError::CrossDeviceLink);
    }

    ext4_sb(&sb).write_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            fs.link(dir.ino().get() as u32, name, target.ino().get() as u32)
                .map_err(map_ext4_error)
        })
    })?;

    target.inode().inc_nlink();
    Ok(())
}

fn ext4_unlink(dir: &InodeRef, name: &str) -> Result<(), FsError> {
    let sb = dir.sb();
    let child_ino = ext4_sb(&sb).write_tx(|| {
        let (ino, ty) = ext4_lookup_child(dir, name)?;
        if ty == InodeType::Dir {
            return Err(FsError::IsDir);
        }
        ext4_sb(&sb).with_fs(|fs| {
            fs.unlink(dir.ino().get() as u32, name)
                .map_err(map_ext4_error)
        })?;
        Ok(ino)
    })?;

    if let Some(inode) = sb.try_iget(child_ino) {
        inode.inode().dec_nlink();
        if inode.nlink() == 0 {
            sb.unindex_inode(inode.inode());
        }
        drop(inode);
    }

    Ok(())
}

fn ext4_rmdir(dir: &InodeRef, name: &str) -> Result<(), FsError> {
    let sb = dir.sb();
    let child_ino = ext4_sb(&sb).write_tx(|| {
        let (ino, ty) = ext4_lookup_child(dir, name)?;
        if ty != InodeType::Dir {
            return Err(FsError::NotDir);
        }
        ext4_sb(&sb).with_fs(|fs| {
            // lwext4_rust already handles the case when the target to unlink is a
            // directory, so we don't need to do extra work here. though this
            // may seem a bit weird...
            fs.unlink(dir.ino().get() as u32, name)
                .map_err(map_ext4_error)
        })?;
        Ok(ino)
    })?;

    dir.inode().dec_nlink();

    if let Some(inode) = sb.try_iget(child_ino) {
        ext4_sync_cached_nlink(inode.inode(), 0);
        sb.unindex_inode(inode.inode());
        drop(inode);
    }

    Ok(())
}

fn ext4_read_link(inode: &InodeRef) -> Result<PathBuf, FsError> {
    let sb = inode.sb();

    let bytes = ext4_get_attr(inode)?.size as usize;

    let mut buf = vec![0u8; bytes];

    ext4_sb(&sb).read_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            fs.read_at(inode.ino().get() as u32, &mut buf, 0)
                .map_err(map_ext4_error)
        })
    })?;

    let s = core::str::from_utf8(&buf).map_err(|_| FsError::InvalidPath)?;

    Ok(PathBuf::from(s))
}

pub(super) static EXT4_DIR_INODE_OPS: InodeOps = InodeOps {
    lookup: ext4_lookup,
    create: ext4_create,
    symlink: ext4_symlink,
    link: ext4_link,
    unlink: ext4_unlink,
    rmdir: ext4_rmdir,
    open: ext4_open,
    read_link: |_| Err(FsError::NotSymlink),
    get_attr: ext4_get_attr,
};

pub(super) static EXT4_REG_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(FsError::NotSupported),
    create: |_, _, _| Err(FsError::NotDir),
    symlink: |_, _, _| Err(FsError::NotDir),
    link: |_, _, _| Err(FsError::NotDir),
    unlink: |_, _| Err(FsError::NotDir),
    rmdir: |_, _| Err(FsError::NotDir),
    open: ext4_open,
    read_link: |_| Err(FsError::NotSymlink),
    get_attr: ext4_get_attr,
};

pub(super) static EXT4_DEV_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(FsError::NotSupported),
    create: |_, _, _| Err(FsError::NotDir),
    symlink: |_, _, _| Err(FsError::NotDir),
    link: |_, _, _| Err(FsError::NotDir),
    unlink: |_, _| Err(FsError::NotDir),
    rmdir: |_, _| Err(FsError::NotDir),
    open: |_| Err(FsError::NotSupported),
    read_link: |_| Err(FsError::NotSymlink),
    get_attr: ext4_get_attr,
};

pub(super) static EXT4_SYMLINK_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(FsError::NotSupported),
    create: |_, _, _| Err(FsError::NotDir),
    symlink: |_, _, _| Err(FsError::NotDir),
    link: |_, _, _| Err(FsError::NotDir),
    unlink: |_, _| Err(FsError::NotDir),
    rmdir: |_, _| Err(FsError::NotDir),
    open: |_| Err(FsError::NotSupported),
    read_link: ext4_read_link,
    get_attr: ext4_get_attr,
};
