use core::fmt::Write as _;

use crate::{
    fs::{
        iomux::PollEvent,
        proc::{
            read_snapshot_at,
            tgid::{
                TgidEntry, default_tgid_entry_prv,
                stat::{proc_comm, proc_state},
                validate_tgid_sub_inode,
            },
        },
    },
    prelude::*,
    sched::config::CpuMask,
    task::sig::set::SigSet,
    utils::any_opaque::NilOpaque,
};

fn tgid_status_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let _binding = validate_tgid_sub_inode(inode)?;

    Ok(OpenedFile::new(&TGID_STATUS_FILE_OPS, NilOpaque::new()))
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

fn tgid_status_read(
    file: &File,
    pos: &mut usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let data = build_status_text(file.inode())?;

    if *pos >= data.len() {
        return Ok(0);
    }

    let to_read = usize::min(buf.len(), data.len() - *pos);
    buf[..to_read].copy_from_slice(&data.as_bytes()[*pos..*pos + to_read]);
    *pos += to_read;

    Ok(to_read)
}

fn tgid_status_read_at(
    file: &File,
    pos: usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let data = build_status_text(file.inode())?;

    read_snapshot_at(pos, buf, data.as_bytes())
}

fn tgid_status_seek(file: &File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError> {
    let data = build_status_text(file.inode())?;

    seek_with_bounded_size(file, pos, from, data.len())
}

static TGID_STATUS_FILE_OPS: FileOps = FileOps {
    read: tgid_status_read,
    write: |_, _, _, _| Err(SysError::NotSupported),
    read_at: tgid_status_read_at,
    write_at: |_, _, _, _| Err(SysError::NotSupported),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: tgid_status_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

pub static TGID_STATUS_TGID_ENTRY: TgidEntry = TgidEntry {
    name: "status",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    inode_ops: &TGID_STATUS_INODE_OPS,
    make_prv: default_tgid_entry_prv,
};

fn build_status_text(inode: &InodeRef) -> Result<String, SysError> {
    let binding = validate_tgid_sub_inode(inode)?;
    let tg = &binding.tg;
    let leader = tg.leader().ok_or(SysError::NoSuchProcess)?;
    let is_kthread = tg.ty() == ThreadGroupType::KThread;

    let pid = leader.tid().get();
    let tgid = tg.tgid().get();
    let display = tg.proc_display_parentage();
    let ppid = display.ppid.get();
    let pgrp = display.pgrp.get();
    let session = display.session.get();
    let cred = leader.cred();
    let vsize_kb = if is_kthread {
        0
    } else {
        bytes_to_kb(leader.clone_uspace_handle().lock().vsize_bytes())
    };
    let sig_pnd = leader.pending_signal_set();
    let shd_pnd = tg.shared_pending_signal_set();
    let sig_blk = leader.snapshot_current_sig_mask();
    let (sig_ign, sig_cgt) = tg
        .signal_disposition()
        .map(|disp| {
            let disp = disp.read();
            (disp.ignored_signals(), disp.caught_signals())
        })
        .unwrap_or((SigSet::new(), SigSet::new()));
    let pending_count = sig_pnd.union(&shd_pnd).as_u64().count_ones();
    let affinity = leader.sched_config().affinity();
    let cpu_list = cpus_allowed_list(affinity);
    let cpu_mask = cpus_allowed_mask(affinity);
    let kthread = is_kthread as u8;
    let state = proc_state(tg, &leader);

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
    writeln!(out, "State:\t{} ({})", state.character(), state.name()).unwrap();
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
    writeln!(out, "Cpus_allowed:\t{}", cpu_mask).unwrap();
    writeln!(out, "Cpus_allowed_list:\t{}", cpu_list).unwrap();
    writeln!(out, "Mems_allowed:\t1").unwrap();
    writeln!(out, "Mems_allowed_list:\t0").unwrap();
    writeln!(out, "voluntary_ctxt_switches:\t0").unwrap();
    writeln!(out, "nonvoluntary_ctxt_switches:\t0").unwrap();

    Ok(out)
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

fn cpus_allowed_mask(affinity: CpuMask) -> String {
    let mut nibbles = vec![0u8; usize::max(16, (MAX_LOGICAL_CPUS + 3) / 4)];
    for cpu in affinity.iter() {
        let cpu = cpu.logical_id();
        nibbles[cpu / 4] |= 1 << (cpu % 4);
    }

    let mut out = String::with_capacity(nibbles.len());
    for nibble in nibbles.into_iter().rev() {
        out.push(char::from_digit(nibble.into(), 16).unwrap());
    }
    out
}

fn cpus_allowed_list(affinity: CpuMask) -> String {
    let cpus: Vec<_> = affinity.iter().map(|cpu| cpu.logical_id()).collect();
    assert!(!cpus.is_empty(), "effective affinity must not be empty");
    let mut out = String::new();
    let mut start = cpus[0];
    let mut end = start;
    for cpu in cpus.into_iter().skip(1) {
        if cpu == end + 1 {
            end = cpu;
            continue;
        }
        append_cpu_range(&mut out, start, end);
        start = cpu;
        end = cpu;
    }
    append_cpu_range(&mut out, start, end);
    out
}

fn append_cpu_range(out: &mut String, start: usize, end: usize) {
    if !out.is_empty() {
        out.push(',');
    }
    if start == end {
        write!(out, "{}", start).unwrap();
    } else {
        write!(out, "{}-{}", start, end).unwrap();
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn mask(cpus: impl IntoIterator<Item = usize>) -> CpuMask {
        let mut mask = CpuMask::empty();
        for cpu in cpus {
            mask.insert(CpuId::new(cpu));
        }
        mask
    }

    fn bit(text: &str, cpu: usize) -> bool {
        let digit = text.as_bytes()[text.len() - 1 - cpu / 4];
        let value = char::from(digit).to_digit(16).unwrap() as u8;
        value & (1 << (cpu % 4)) != 0
    }

    #[kunit]
    fn test_cpus_allowed_mask_formats_sparse_and_full_compile_time_domain() {
        let width = usize::max(16, (MAX_LOGICAL_CPUS + 3) / 4);
        let sparse = mask([0, MAX_LOGICAL_CPUS - 1]);
        let sparse_text = cpus_allowed_mask(sparse);
        assert_eq!(sparse_text.len(), width);
        for cpu in 0..width * 4 {
            assert_eq!(
                bit(&sparse_text, cpu),
                cpu == 0 || cpu == MAX_LOGICAL_CPUS - 1
            );
        }

        let full = cpus_allowed_mask(CpuMask::all());
        assert_eq!(full.len(), width);
        for cpu in 0..width * 4 {
            assert_eq!(bit(&full, cpu), cpu < MAX_LOGICAL_CPUS);
        }
    }

    #[kunit]
    fn test_cpus_allowed_list_formats_sparse_ranges_and_full_domain() {
        assert_eq!(cpus_allowed_list(mask([0])), "0");
        if MAX_LOGICAL_CPUS >= 4 {
            assert_eq!(cpus_allowed_list(mask([0, 1, 3])), "0-1,3");
        }
        if MAX_LOGICAL_CPUS >= 2 {
            assert_eq!(
                cpus_allowed_list(CpuMask::all()),
                format!("0-{}", MAX_LOGICAL_CPUS - 1)
            );
        }
    }
}
