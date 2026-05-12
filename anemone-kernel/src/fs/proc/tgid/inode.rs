use crate::{fs::proc::tgid::file::TGID_FILE_OPS, utils::any_opaque::NilOpaque};

use super::*;

fn tgid_lookup(inode: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
    let binding = validate_tgid_inode(inode)?;

    let prv = tgid_inode_private(inode);

    if let Some(entry) = find_tgid_entry_by_name(name) {
        let mut sub_ino = prv.sub_ino.lock();
        if let Some(SubInoRecord { ino, instantiated }) = sub_ino.get_mut(entry.name) {
            if !*instantiated {
                // ino allocated, but inode not instantiated. let's instantiate it.
                let inode = entry.new_inode(binding, inode.sb().clone(), Some(*ino));
                let inode = inode.sb().seed_inode(Arc::new(inode));
                *instantiated = true;
                Ok(inode)
            } else {
                // already instantiated, just return it.
                Ok(inode
                    .sb()
                    .try_iget(*ino)
                    .expect("sub inode should exist if its ino is recorded"))
            }
        } else {
            // oops. first time accessed. let's create it.
            let inode = entry.new_inode(binding, inode.sb().clone(), None);

            sub_ino.insert(
                entry.name,
                SubInoRecord {
                    ino: inode.ino(),
                    instantiated: true,
                },
            );

            // don't forget to seed the inode, so that it can be `try_iget`ed later.
            let inode = inode.sb().seed_inode(Arc::new(inode));

            Ok(inode)
        }
    } else {
        Err(SysError::NotFound)
    }
}

fn tgid_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let _binding = validate_tgid_inode(inode)?;

    Ok(OpenedFile {
        file_ops: &TGID_FILE_OPS,
        prv: NilOpaque::new(),
    })
}

fn tgid_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let _binding = validate_tgid_inode(inode)?;

    let now = Instant::now().to_duration();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: inode.nlink(),
        uid: 0,
        gid: 0,
        rdev: DeviceId::None,
        size: 0,
        atime: now,
        mtime: now,
        ctime: now,
    })
}

pub static TGID_INODE_OPS: InodeOps = InodeOps {
    lookup: tgid_lookup,
    touch: |_, _, _| Err(SysError::NotSupported),
    mkdir: |_, _, _| Err(SysError::NotSupported),
    symlink: |_, _, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::IsDir),
    unlink: |_, _| Err(SysError::IsDir),
    rmdir: |_, _| Err(SysError::NotSupported),
    open: tgid_open,
    read_link: |_| Err(SysError::IsDir),
    get_attr: tgid_get_attr,
};
