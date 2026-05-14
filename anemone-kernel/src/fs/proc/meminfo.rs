use crate::{fs::proc::pde::ProcDirEntry, prelude::*, utils::any_opaque::NilOpaque};

fn proc_meminfo_open(_inode: &InodeRef) -> Result<OpenedFile, SysError> {
    Ok(OpenedFile {
        file_ops: &PROC_MEMINFO_FILE_OPS,
        prv: NilOpaque::new(),
    })
}

fn proc_meminfo_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let now = Instant::now().to_duration();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: 1,
        uid: 0,
        gid: 0,
        rdev: DeviceId::None,
        size: 0,
        atime: now,
        mtime: now,
        ctime: now,
    })
}

static PROC_MEMINFO_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: proc_meminfo_open,
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: proc_meminfo_get_attr,
};

fn meminfo_string() -> String {
    // current a fake implementation.

    let total_mem = format!("MemTotal:\t{} Kb\n", 393939);
    let free_mem = format!("MemFree:\t{} Kb\n", 393939);
    let available_mem = format!("MemAvailable:\t{} Kb\n", 393939);
    let buffers = format!("Buffers:\t{} Kb\n", 393939);
    let cached = format!("Cached:\t{} Kb\n", 393939);
    let swap_total = format!("SwapTotal:\t{} Kb\n", 0);
    let swap_free = format!("SwapFree:\t{} Kb\n", 0);
    let swap_cached = format!("SwapCached:\t{} Kb\n", 0);
    let shmem = format!("Shmem:\t{} Kb\n", 0);
    let slab = format!("Slab:\t{} Kb\n", 393939);

    total_mem
        + &free_mem
        + &available_mem
        + &buffers
        + &cached
        + &swap_total
        + &swap_cached
        + &swap_free
        + &shmem
        + &slab
}

fn proc_meminfo_read(file: &File, pos: &mut usize, buf: &mut [u8]) -> Result<usize, SysError> {
    let meminfo_string = meminfo_string();
    let meminfo_bytes = meminfo_string.as_bytes();

    if *pos >= meminfo_bytes.len() {
        return Ok(0);
    }

    let to_read = usize::min(buf.len(), meminfo_bytes.len() - *pos);
    buf[..to_read].copy_from_slice(&meminfo_bytes[*pos..*pos + to_read]);
    *pos += to_read;

    Ok(to_read)
}

fn proc_meminfo_validate_seek(_file: &File, pos: usize) -> Result<(), SysError> {
    let meminfo_string = meminfo_string();
    let meminfo_bytes = meminfo_string.as_bytes();

    if pos > meminfo_bytes.len() {
        return Err(SysError::InvalidArgument);
    }

    Ok(())
}

static PROC_MEMINFO_FILE_OPS: FileOps = FileOps {
    read: proc_meminfo_read,
    write: |_, _, _| Err(SysError::NotSupported),
    validate_seek: proc_meminfo_validate_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, _| Ok(PollEvent::READABLE),
};

pub static PROC_MEMINFO_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "meminfo",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    ops: &PROC_MEMINFO_INODE_OPS,
    ino: unsafe { MonoOnce::new() },
};
