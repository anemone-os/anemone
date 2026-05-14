use anemone_abi::errno::ENOENT;
use lwext4_rust::InodeType as LwExt4InodeType;

use crate::{
    fs::{
        ext4::{
            ext4_ino, ext4_sb, file::EXT4_SYMLINK_FILE_OPS, map_ext4_error, map_lwext4_inode_type,
            map_vfs_inode_type,
        },
        inode::RenameFlags,
    },
    prelude::*,
    utils::any_opaque::NilOpaque,
};

use super::file::{EXT4_DIR_FILE_OPS, EXT4_REG_FILE_OPS};

fn ext4_lookup_child(dir: &InodeRef, name: &str) -> Result<(Ino, InodeType), SysError> {
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

fn ext4_create_child(
    dir: &InodeRef,
    name: &str,
    ty: InodeType,
    perm: InodePerm,
) -> Result<InodeRef, SysError> {
    let sb = dir.sb();
    let raw_ino = ext4_sb(&sb).write_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            match fs.lookup(dir.ino().get() as u32, name) {
                Ok(_) => return Err(SysError::AlreadyExists),
                Err(err) if err.code == ENOENT as i32 => {},
                Err(err) => return Err(map_ext4_error(err)),
            }

            fs.create(
                dir.ino().get() as u32,
                name,
                map_vfs_inode_type(ty)?,
                perm.bits() as u32,
            )
            .map_err(map_ext4_error)
        })
    })?;
    let ino = ext4_ino(raw_ino).expect("internal error: lwext4 returned invalid inode number");

    if ty == InodeType::Dir {
        dir.inode().inc_nlink();
    }
    Ok(sb.iget(ino)?)
}

fn ext4_touch(dir: &InodeRef, name: &str, perm: InodePerm) -> Result<InodeRef, SysError> {
    ext4_create_child(dir, name, InodeType::Regular, perm)
}

fn ext4_mkdir(dir: &InodeRef, name: &str, perm: InodePerm) -> Result<InodeRef, SysError> {
    ext4_create_child(dir, name, InodeType::Dir, perm)
}

fn ext4_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let file_ops = match inode.ty() {
        InodeType::Dir => &EXT4_DIR_FILE_OPS,
        InodeType::Regular => &EXT4_REG_FILE_OPS,
        InodeType::Symlink => &EXT4_SYMLINK_FILE_OPS,
        InodeType::Fifo => unimplemented!("ext4 fifo file"),
        InodeType::Char | InodeType::Block => unimplemented!("ext4 dev file"),
    };

    Ok(OpenedFile {
        file_ops,
        prv: NilOpaque::new(),
    })
}

#[inline]
fn ext4_mode_from_attr(node_type: LwExt4InodeType, raw_mode: u32) -> Result<InodeMode, SysError> {
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

fn ext4_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
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

    let meta = inode.inode().meta_snapshot();

    Ok(InodeStat {
        fs_dev: ext4_fs_dev(&sb),
        ino: inode.ino(),
        mode: InodeMode::new(inode.ty(), inode.perm()),
        nlink: meta.nlink,
        uid: attr.uid,
        gid: attr.gid,
        rdev: DeviceId::None,
        size: meta.size,
        atime: meta.atime,
        mtime: meta.mtime,
        ctime: meta.ctime,
    })
}

fn ext4_lookup(dir: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
    let sb = dir.sb();
    let (ino, _ty) = ext4_sb(&sb).read_tx(|| ext4_lookup_child(dir, name))?;
    dir.sb().iget(ino)
}

fn ext4_symlink(dir: &InodeRef, name: &str, target: &Path) -> Result<InodeRef, SysError> {
    let sb = dir.sb();
    let raw_ino = ext4_sb(&sb).write_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            match fs.lookup(dir.ino().get() as u32, name) {
                Ok(_) => return Err(SysError::AlreadyExists),
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

fn ext4_link(dir: &InodeRef, name: &str, target: &InodeRef) -> Result<(), SysError> {
    if target.ty() == InodeType::Dir {
        return Err(SysError::IsDir);
    }

    let sb = dir.sb();
    if !Arc::ptr_eq(&sb, &target.sb()) {
        return Err(SysError::CrossDeviceLink);
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

fn ext4_unlink(dir: &InodeRef, name: &str) -> Result<(), SysError> {
    let sb = dir.sb();
    let child_ino = ext4_sb(&sb).write_tx(|| {
        let (ino, ty) = ext4_lookup_child(dir, name)?;
        if ty == InodeType::Dir {
            return Err(SysError::IsDir);
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

fn ext4_rmdir(dir: &InodeRef, name: &str) -> Result<(), SysError> {
    let sb = dir.sb();
    let child_ino = ext4_sb(&sb).write_tx(|| {
        let (ino, ty) = ext4_lookup_child(dir, name)?;
        if ty != InodeType::Dir {
            return Err(SysError::NotDir);
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
        inode.inode().set_nlink(0);
        sb.unindex_inode(inode.inode());
        drop(inode);
    }

    Ok(())
}

fn ext4_rename(
    old_dir: &InodeRef,
    old_name: &str,
    new_dir: &InodeRef,
    new_name: &str,
    flags: RenameFlags,
) -> Result<(), SysError> {
    enum RenameOutcome {
        NoOp,
        Renamed {
            src_ty: InodeType,
            overwritten: Option<(Ino, InodeType)>,
        },
    }

    if old_dir == new_dir && old_name == new_name {
        return Ok(());
    }

    let sb = old_dir.sb();
    if !Arc::ptr_eq(&sb, &new_dir.sb()) {
        return Err(SysError::CrossDeviceLink);
    }

    let outcome = ext4_sb(&sb).write_tx(|| {
        let (src_ino, src_ty) = ext4_lookup_child(old_dir, old_name)?;

        let overwritten = match ext4_lookup_child(new_dir, new_name) {
            Ok((ino, ty)) => Some((ino, ty)),
            Err(SysError::NotFound) => None,
            Err(err) => return Err(err),
        };

        if overwritten.is_some() && flags.contains(RenameFlags::NO_REPLACE) {
            return Err(SysError::AlreadyExists);
        }

        if let Some((dst_ino, dst_ty)) = overwritten {
            if dst_ino == src_ino {
                return Ok(RenameOutcome::NoOp);
            }

            match (src_ty, dst_ty) {
                (InodeType::Dir, ty) if ty != InodeType::Dir => {
                    return Err(SysError::NotDir);
                },
                (ty, InodeType::Dir) if ty != InodeType::Dir => {
                    return Err(SysError::IsDir);
                },
                _ => {},
            }
        }

        ext4_sb(&sb).with_fs(|fs| {
            fs.rename(
                old_dir.ino().get() as u32,
                old_name,
                new_dir.ino().get() as u32,
                new_name,
            )
            .map_err(map_ext4_error)
        })?;

        Ok(RenameOutcome::Renamed {
            src_ty,
            overwritten,
        })
    })?;

    let RenameOutcome::Renamed {
        src_ty,
        overwritten,
    } = outcome
    else {
        return Ok(());
    };

    if let Some((dst_ino, dst_ty)) = overwritten {
        if dst_ty == InodeType::Dir {
            new_dir.inode().dec_nlink();
        }

        if let Some(inode) = sb.try_iget(dst_ino) {
            if dst_ty == InodeType::Dir {
                inode.inode().set_nlink(0);
                sb.unindex_inode(inode.inode());
            } else {
                inode.inode().dec_nlink();
                if inode.nlink() == 0 {
                    sb.unindex_inode(inode.inode());
                }
            }
            drop(inode);
        }
    }

    if src_ty == InodeType::Dir && old_dir != new_dir {
        old_dir.inode().dec_nlink();
        new_dir.inode().inc_nlink();
    }

    Ok(())
}

fn ext4_read_link(inode: &InodeRef) -> Result<PathBuf, SysError> {
    let sb = inode.sb();

    let bytes = ext4_get_attr(inode)?.size as usize;

    let mut buf = vec![0u8; bytes];

    ext4_sb(&sb).read_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            fs.read_at(inode.ino().get() as u32, &mut buf, 0)
                .map_err(map_ext4_error)
        })
    })?;

    let s = core::str::from_utf8(&buf).map_err(|_| SysError::InvalidPath)?;

    Ok(PathBuf::from(s))
}

pub(super) static EXT4_DIR_INODE_OPS: InodeOps = InodeOps {
    lookup: ext4_lookup,
    touch: ext4_touch,
    mkdir: ext4_mkdir,
    symlink: ext4_symlink,
    link: ext4_link,
    unlink: ext4_unlink,
    rmdir: ext4_rmdir,
    open: ext4_open,
    rename: ext4_rename,
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: ext4_get_attr,
};

pub(super) static EXT4_REG_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotSupported),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: ext4_open,
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: ext4_get_attr,
};

pub(super) static EXT4_DEV_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotSupported),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: ext4_get_attr,
};

pub(super) static EXT4_SYMLINK_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotSupported),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| Err(SysError::NotSupported),
    read_link: ext4_read_link,
    get_attr: ext4_get_attr,
};
