use core::fmt::Write as _;

use crate::{
    fs::{
        iomux::PollEvent,
        proc::tgid::{
            TgidEntry,
            stat::{proc_comm, proc_state},
            validate_tgid_sub_inode,
        },
    },
    prelude::*,
    task::sig::set::SigSet,
    utils::any_opaque::NilOpaque,
};

fn tgid_status_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let _binding = validate_tgid_sub_inode(inode)?;

    Ok(OpenedFile {
        file_ops: &TGID_STATUS_FILE_OPS,
        prv: NilOpaque::new(),
    })
}

fn tgid_status_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
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

static TGID_STATUS_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: tgid_status_open,
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: tgid_status_get_attr,
};

fn tgid_status_read(file: &File, pos: &mut usize, buf: &mut [u8]) -> Result<usize, SysError> {
    let data = build_status_text(file.inode())?;

    if *pos >= data.len() {
        return Ok(0);
    }

    let to_read = usize::min(buf.len(), data.len() - *pos);
    buf[..to_read].copy_from_slice(&data.as_bytes()[*pos..*pos + to_read]);
    *pos += to_read;

    Ok(to_read)
}

fn tgid_status_validate_seek(file: &File, pos: usize) -> Result<(), SysError> {
    let data = build_status_text(file.inode())?;

    if pos > data.len() {
        return Err(SysError::InvalidArgument);
    }

    Ok(())
}

static TGID_STATUS_FILE_OPS: FileOps = FileOps {
    read: tgid_status_read,
    write: |_, _, _| Err(SysError::NotSupported),
    validate_seek: tgid_status_validate_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

pub static TGID_STATUS_TGID_ENTRY: TgidEntry = TgidEntry {
    name: "status",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    inode_ops: &TGID_STATUS_INODE_OPS,
};

fn build_status_text(inode: &InodeRef) -> Result<String, SysError> {
    let binding = validate_tgid_sub_inode(inode)?;
    let tg = &binding.tg;
    let leader = tg.leader().ok_or(SysError::NoSuchProcess)?;

    let pid = leader.tid().get();
    let tgid = tg.tgid().get();
    let ppid = tg.parent_tgid().map(|tid| tid.get()).unwrap_or(0);
    let pgrp = tg.pgid().get();
    let session = tg.sid().get();
    let cred = leader.cred();
    let vsize_kb = leader
        .try_clone_uspace_handle()
        .map(|usp| bytes_to_kb(usp.lock().vsize_bytes()))
        .unwrap_or(0);
    let sig_pnd = leader.pending_signal_set();
    let shd_pnd = tg.shared_pending_signal_set();
    let sig_blk = leader.sig_mask();
    let (sig_ign, sig_cgt) = tg
        .signal_disposition()
        .map(|disp| {
            let disp = disp.read();
            (disp.ignored_signals(), disp.caught_signals())
        })
        .unwrap_or((SigSet::new(), SigSet::new()));
    let pending_count = sig_pnd.union(&shd_pnd).as_u64().count_ones();
    let cpu_list = cpus_allowed_list();
    let cpu_mask = cpus_allowed_mask();
    let kthread = leader.flags().is_kernel() as u8;

    // Stage-1 placeholders:
    // - no resident, locked, per-segment, or page-table accounting is wired yet.
    // - no ptrace, pid namespace, cpuset, NUMA, or seccomp model is wired yet.
    // - FDSize reports the configured fd-table capacity, not Linux's dynamic
    //   fdtable growth watermark.
    let vm_peak_kb = vsize_kb;
    let vm_size_kb = vsize_kb;
    let vm_lck_kb = 0;
    let vm_pin_kb = 0;
    let vm_hwm_kb = 0;
    let vm_rss_kb = 0;
    let rss_anon_kb = 0;
    let rss_file_kb = 0;
    let rss_shmem_kb = 0;
    let vm_data_kb = 0;
    let vm_stk_kb = 0;
    let vm_exe_kb = 0;
    let vm_lib_kb = 0;
    let vm_pte_kb = 0;
    let vm_swap_kb = 0;
    let hugetlb_pages_kb = 0;
    let tracer_pid = 0;
    let ngid = 0;
    let sigq_limit = 0;

    let mut out = String::new();
    writeln!(out, "Name:\t{}", proc_comm(&leader)).unwrap();
    writeln!(
        out,
        "State:\t{} ({})",
        proc_state(&leader),
        proc_state_name(&leader)
    )
    .unwrap();
    writeln!(out, "Tgid:\t{}", tgid).unwrap();
    writeln!(out, "Ngid:\t{}", ngid).unwrap();
    writeln!(out, "Pid:\t{}", pid).unwrap();
    writeln!(out, "PPid:\t{}", ppid).unwrap();
    writeln!(out, "TracerPid:\t{}", tracer_pid).unwrap();
    writeln!(out, "Uid:\t{}", ids_line(cred.uid)).unwrap();
    writeln!(out, "Gid:\t{}", ids_line(cred.gid)).unwrap();
    writeln!(out, "FDSize:\t{}", MAX_FD_PER_PROCESS).unwrap();
    write!(out, "Groups:\t").unwrap();
    for (idx, group) in cred.groups.iter().enumerate() {
        if idx > 0 {
            write!(out, " ").unwrap();
        }
        write!(out, "{}", group.get()).unwrap();
    }
    writeln!(out).unwrap();
    writeln!(out, "NStgid:\t{}", tgid).unwrap();
    writeln!(out, "NSpid:\t{}", pid).unwrap();
    writeln!(out, "NSpgid:\t{}", pgrp).unwrap();
    writeln!(out, "NSsid:\t{}", session).unwrap();
    writeln!(out, "Kthread:\t{}", kthread).unwrap();
    writeln!(out, "VmPeak:\t{:>8} kB", vm_peak_kb).unwrap();
    writeln!(out, "VmSize:\t{:>8} kB", vm_size_kb).unwrap();
    writeln!(out, "VmLck:\t{:>8} kB", vm_lck_kb).unwrap();
    writeln!(out, "VmPin:\t{:>8} kB", vm_pin_kb).unwrap();
    writeln!(out, "VmHWM:\t{:>8} kB", vm_hwm_kb).unwrap();
    writeln!(out, "VmRSS:\t{:>8} kB", vm_rss_kb).unwrap();
    writeln!(out, "RssAnon:\t{:>8} kB", rss_anon_kb).unwrap();
    writeln!(out, "RssFile:\t{:>8} kB", rss_file_kb).unwrap();
    writeln!(out, "RssShmem:\t{:>8} kB", rss_shmem_kb).unwrap();
    writeln!(out, "VmData:\t{:>8} kB", vm_data_kb).unwrap();
    writeln!(out, "VmStk:\t{:>8} kB", vm_stk_kb).unwrap();
    writeln!(out, "VmExe:\t{:>8} kB", vm_exe_kb).unwrap();
    writeln!(out, "VmLib:\t{:>8} kB", vm_lib_kb).unwrap();
    writeln!(out, "VmPTE:\t{:>8} kB", vm_pte_kb).unwrap();
    writeln!(out, "VmSwap:\t{:>8} kB", vm_swap_kb).unwrap();
    writeln!(out, "HugetlbPages:\t{:>8} kB", hugetlb_pages_kb).unwrap();
    writeln!(out, "CoreDumping:\t0").unwrap();
    writeln!(out, "THP_enabled:\t0").unwrap();
    writeln!(out, "Threads:\t{}", tg.ntasks()).unwrap();
    writeln!(out, "SigQ:\t{}/{}", pending_count, sigq_limit).unwrap();
    writeln!(out, "SigPnd:\t{}", sigset_hex(sig_pnd)).unwrap();
    writeln!(out, "ShdPnd:\t{}", sigset_hex(shd_pnd)).unwrap();
    writeln!(out, "SigBlk:\t{}", sigset_hex(sig_blk)).unwrap();
    writeln!(out, "SigIgn:\t{}", sigset_hex(sig_ign)).unwrap();
    writeln!(out, "SigCgt:\t{}", sigset_hex(sig_cgt)).unwrap();
    writeln!(out, "CapInh:\t{:016x}", cred.caps.inheritable().bits()).unwrap();
    writeln!(out, "CapPrm:\t{:016x}", cred.caps.permitted().bits()).unwrap();
    writeln!(out, "CapEff:\t{:016x}", cred.caps.effective().bits()).unwrap();
    writeln!(out, "CapBnd:\t{:016x}", cred.caps.bounding().bits()).unwrap();
    writeln!(out, "CapAmb:\t{:016x}", cred.caps.ambient().bits()).unwrap();
    writeln!(out, "NoNewPrivs:\t{}", leader.no_new_privs() as u8).unwrap();
    writeln!(out, "Seccomp:\t0").unwrap();
    writeln!(out, "Seccomp_filters:\t0").unwrap();
    writeln!(out, "Cpus_allowed:\t{:016x}", cpu_mask).unwrap();
    writeln!(out, "Cpus_allowed_list:\t{}", cpu_list).unwrap();
    writeln!(out, "Mems_allowed:\t1").unwrap();
    writeln!(out, "Mems_allowed_list:\t0").unwrap();
    writeln!(out, "voluntary_ctxt_switches:\t0").unwrap();
    writeln!(out, "nonvoluntary_ctxt_switches:\t0").unwrap();

    Ok(out)
}

fn proc_state_name(leader: &Task) -> &'static str {
    match leader.status() {
        TaskStatus::Runnable => "running",
        TaskStatus::Zombie => "zombie",
        TaskStatus::Waiting {
            interruptible: true,
        } => "sleeping",
        TaskStatus::Waiting {
            interruptible: false,
        } => "disk sleep",
    }
}

fn ids_line<T: UserId>(ids: Credentials<T>) -> String {
    format!(
        "{}\t{}\t{}\t{}",
        ids.real.get(),
        ids.effective.get(),
        ids.saved.get(),
        ids.fs.get()
    )
}

fn sigset_hex(set: SigSet) -> String {
    format!("{:016x}", set.as_u64())
}

fn bytes_to_kb(bytes: usize) -> usize {
    bytes / 1024
}

fn cpus_allowed_mask() -> u64 {
    let cpu_count = usize::min(ncpus(), 64);
    if cpu_count == 64 {
        u64::MAX
    } else {
        (1u64 << cpu_count) - 1
    }
}

fn cpus_allowed_list() -> String {
    let cpu_count = ncpus();
    if cpu_count <= 1 {
        "0".to_string()
    } else {
        format!("0-{}", cpu_count - 1)
    }
}
