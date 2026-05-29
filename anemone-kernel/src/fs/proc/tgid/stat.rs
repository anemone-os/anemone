use crate::{
    fs::{
        iomux::PollEvent,
        proc::tgid::{TgidEntry, validate_tgid_sub_inode},
    },
    prelude::*,
    time::duration_to_ticks,
    utils::any_opaque::NilOpaque,
};

fn tgid_stat_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let _binding = validate_tgid_sub_inode(inode)?;

    Ok(OpenedFile {
        file_ops: &TGID_STAT_FILE_OPS,
        prv: NilOpaque::new(),
    })
}

fn tgid_stat_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let _binding = validate_tgid_sub_inode(inode)?;
    let meta = inode.inode().meta_snapshot();
    let now = Instant::now().to_duration();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: 1,
        uid: meta.uid,
        gid: meta.gid,
        rdev: DeviceId::None,
        size: 0,
        atime: now,
        mtime: now,
        ctime: now,
    })
}

static TGID_STAT_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: tgid_stat_open,
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: tgid_stat_get_attr,
};

fn tgid_stat_read(file: &File, pos: &mut usize, buf: &mut [u8]) -> Result<usize, SysError> {
    let data = build_stat_line(file.inode())?;

    if *pos >= data.len() {
        return Ok(0);
    }

    let to_read = usize::min(buf.len(), data.len() - *pos);
    buf[..to_read].copy_from_slice(&data.as_bytes()[*pos..*pos + to_read]);
    *pos += to_read;

    Ok(to_read)
}

fn tgid_stat_validate_seek(file: &File, pos: usize) -> Result<(), SysError> {
    let data = build_stat_line(file.inode())?;

    if pos > data.len() {
        return Err(SysError::InvalidArgument);
    }

    Ok(())
}

static TGID_STAT_FILE_OPS: FileOps = FileOps {
    read: tgid_stat_read,
    write: |_, _, _| Err(SysError::NotSupported),
    validate_seek: tgid_stat_validate_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, req| Ok(PollEvent::READABLE & req.interests()),
};

pub static TGID_STAT_TGID_ENTRY: TgidEntry = TgidEntry {
    name: "stat",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    inode_ops: &TGID_STAT_INODE_OPS,
};

fn build_stat_line(inode: &InodeRef) -> Result<String, SysError> {
    let binding = validate_tgid_sub_inode(inode)?;
    let tg = &binding.tg;
    let leader = tg.leader().ok_or(SysError::NoSuchProcess)?;

    let cpu_usage = tg.cpu_usage_snapshot();
    let (cmdline_start, cmdline_len, env_start, env_len, vsize) =
        if let Some(usp_handle) = leader.try_clone_uspace_handle() {
            let usp = usp_handle.lock();
            let (cmdline_start, cmdline_len) = usp.cmdline_range();
            let (env_start, env_len) = usp.env_range();
            let vsize = usp.vsize_bytes() as u64;
            (cmdline_start, cmdline_len, env_start, env_len, vsize)
        } else {
            (VirtAddr::new(0), 0, VirtAddr::new(0), 0, 0)
        };

    let pid = tg.tgid().get();
    let comm = proc_comm(&leader);
    let state = proc_state(&leader);
    let ppid = tg.parent_tgid().map(|tid| tid.get()).unwrap_or(0);
    let pgrp = tg.pgid().get();
    let session = tg.sid().get();
    let num_threads = tg.ntasks();
    let utime = duration_to_ticks(cpu_usage.self_user());
    let stime = duration_to_ticks(cpu_usage.self_kernel());
    let cutime = duration_to_ticks(cpu_usage.reaped_user());
    let cstime = duration_to_ticks(cpu_usage.reaped_kernel());
    let starttime = leader.create_instant().to_ticks();
    let processor = leader.cpuid().get();
    let exit_signal = tg
        .terminate_signal()
        .map(|sig| sig.as_usize() as i32)
        .unwrap_or(0);
    let exit_code = tg.exit_code().map(exit_code_raw).unwrap_or(0);
    let cmdline_start = cmdline_start.get();
    let cmdline_end = cmdline_start + cmdline_len as u64;
    let env_start = env_start.get();
    let env_end = env_start + env_len as u64;

    // Stage-1 placeholders:
    // - tty/fault/rss/ELF-segment/signal/realtime/delay/guest fields are kept
    //   parse-compatible even though their backing accounting is not wired yet.
    // - rss intentionally stays 0 until resident accounting is available.
    let tty_nr = 0;
    let tpgid = 0;
    let flags = 0;
    let minflt = 0;
    let cminflt = 0;
    let majflt = 0;
    let cmajflt = 0;
    let priority = 20;
    let nice = 0;
    let itrealvalue = 0;
    let rss = 0;
    let rsslim = 0;
    let startcode = 0;
    let endcode = 0;
    let startstack = 0;
    let kstkesp = 0;
    let kstkeip = 0;
    let signal = 0;
    let blocked = 0;
    let sigignore = 0;
    let sigcatch = 0;
    let wchan = 0;
    let nswap = 0;
    let cnswap = 0;
    let rt_priority = 0;
    let policy = 0;
    let delayacct_blkio_ticks = 0;
    let guest_time = 0;
    let cguest_time = 0;
    let start_data = 0;
    let end_data = 0;
    let start_brk = 0;

    Ok(format!(
        "{} ({}) {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}\n",
        pid,
        comm,
        state,
        ppid,
        pgrp,
        session,
        tty_nr,
        tpgid,
        flags,
        minflt,
        cminflt,
        majflt,
        cmajflt,
        utime,
        stime,
        cutime,
        cstime,
        priority,
        nice,
        num_threads,
        itrealvalue,
        starttime,
        vsize,
        rss,
        rsslim,
        startcode,
        endcode,
        startstack,
        kstkesp,
        kstkeip,
        signal,
        blocked,
        sigignore,
        sigcatch,
        wchan,
        nswap,
        cnswap,
        exit_signal,
        processor,
        rt_priority,
        policy,
        delayacct_blkio_ticks,
        guest_time,
        cguest_time,
        start_data,
        end_data,
        start_brk,
        cmdline_start,
        cmdline_end,
        env_start,
        env_end,
        exit_code,
    ))
}

fn proc_comm(task: &Task) -> String {
    let name = task.name();
    let trimmed = name.strip_prefix("@user/").unwrap_or(&name);
    let trimmed = trimmed.strip_prefix("@kernel/").unwrap_or(trimmed);
    let comm = trimmed.rsplit('/').next().unwrap_or(trimmed);
    comm.chars().take(15).collect()
}

fn proc_state(leader: &Task) -> char {
    match leader.status() {
        TaskStatus::Runnable => 'R',
        TaskStatus::Zombie => 'Z',
        TaskStatus::Waiting {
            interruptible: true,
        } => 'S',
        TaskStatus::Waiting {
            interruptible: false,
        } => 'D',
    }
}

fn exit_code_raw(code: ExitCode) -> i32 {
    match code {
        ExitCode::Exited(code) => (code as i32) << 8,
        ExitCode::Signaled(sig) => sig.as_usize() as i32,
    }
}
