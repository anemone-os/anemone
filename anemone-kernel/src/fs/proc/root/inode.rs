use super::*;
use crate::{
    fs::proc::{
        root::file::PROC_ROOT_FILE_OPS,
        superblock::alloc_ino,
        tgid::{
            binding::{ThreadGroupBinding, binding_tx},
            new_tgid_dir_inode,
        },
    },
    prelude::*,
    utils::any_opaque::NilOpaque,
};

fn proc_root_lookup(dir: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
    if let Some(tgid) = u32::from_str_radix(name, 10).ok() {
        // dynamic part.

        let tgid = Tid::new(tgid);

        let inode = binding_tx(|bindings| {
            if let Some(binding) = bindings.get(&tgid) {
                debug_assert!(binding.alive());

                let inode = dir
                    .sb()
                    .try_iget(binding.ino)
                    // TODO
                    .expect("binding exists, but inode does not exist; this might be cause by that a superblock is mounted after an unmounting, when icache is cleared");
                Ok(inode)
            } else {
                // lazily set up binding.
                if let Some(tg) = get_thread_group(&tgid) {
                    let ino = alloc_ino();
                    // create binding
                    let binding = Arc::new(ThreadGroupBinding {
                        tg,
                        ino,
                        alive: AtomicBool::new(true),
                    });

                    assert!(
                        bindings
                            .insert(binding.tg.tgid(), binding.clone())
                            .is_none(),
                        "binding already exists for tgid {}",
                        tgid
                    );

                    kdebugln!(
                        "proc_root_lookup: bound thread group with tgid {} to inode {}",
                        binding.tg.tgid(),
                        binding.ino
                    );

                    let inode = Arc::new(new_tgid_dir_inode(dir.sb().clone(), ino, binding));
                    let inode = dir.sb().seed_inode(inode);

                    Ok(inode)
                } else {
                    // tgid invalid.
                    return Err(SysError::NotFound);
                }
            }
        })?;

        return Ok(inode);
    }

    // TODO: static part.
    if name == "self" {
        // TODO: make self a real symlink. this is just a workaround.

        let curr_tgid = get_current_task().tgid().get();
        return proc_root_lookup(dir, &curr_tgid.to_string());
    }

    knoticeln!("proc_root_lookup: name={} not found", name);

    Err(SysError::NotFound)
}

fn proc_root_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    Ok(OpenedFile {
        file_ops: &PROC_ROOT_FILE_OPS,
        prv: NilOpaque::new(),
    })
}

fn proc_root_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let now = Instant::now().to_duration();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: PROC_ROOT_INO,
        mode: InodeMode::new(InodeType::Dir, InodePerm::all_rwx()),
        nlink: 3, // TODO: should we calculate this dynamically? it's not hard, but it's too slow.
        uid: 0,
        gid: 0,
        rdev: DeviceId::None,
        size: 0,
        atime: now,
        mtime: now,
        ctime: now,
    })
}

pub static PROC_ROOT_INODE_OPS: InodeOps = InodeOps {
    lookup: proc_root_lookup,
    touch: |_, _, _| Err(SysError::NotSupported),
    mkdir: |_, _, _| Err(SysError::NotSupported),
    symlink: |_, _, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::IsDir),
    unlink: |_, _| Err(SysError::IsDir),
    rmdir: |_, _| Err(SysError::NotSupported),
    open: proc_root_open,
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: proc_root_get_attr,
};
