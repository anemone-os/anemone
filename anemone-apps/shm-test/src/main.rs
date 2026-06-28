#![no_std]
#![no_main]

use core::{
    mem::zeroed,
    ptr::{read_volatile, write_volatile},
    sync::atomic::{AtomicU8, Ordering},
};

use anemone_rs::{
    abi::{
        process::linux::{ipc::*, shm::*, signal::SIGSEGV},
        syscall::{
            linux::{SYS_SETUID, SYS_SHMAT, SYS_SHMCTL, SYS_SHMDT, SYS_SHMGET},
            syscall,
        },
    },
    os::linux::process::{
        MmapFlags, MmapProt, WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork, mmap,
        sched_yield, wait4,
    },
    prelude::*,
    process::process_id,
};

const PAGE_SIZE: usize = 4096;
const WAIT_RETRIES: usize = 1_000_000;
const SHM_LOCKED: u32 = 0o2000;

type TestFn = fn() -> Result<(), Errno>;

const TESTS: &[(&str, TestFn)] = &[
    ("keyed-lookup-and-rmid", test_keyed_lookup_and_rmid),
    ("metadata-stats-and-ctl", test_metadata_stats_and_ctl),
    ("fork-shared-and-detach", test_fork_shared_and_detach),
    ("rounding-and-remap", test_rounding_and_remap),
    ("readonly-and-detach-fault", test_readonly_and_detach_fault),
    ("credential-permissions", test_credential_permissions),
    ("invalid-arguments", test_invalid_arguments),
];

fn test_key(salt: i32) -> i32 {
    let pid = process_id() as i32;
    0x5100_0000 | ((pid & 0x7fff) << 8) | (salt & 0xff)
}

fn unique_shmget(key: i32, size: usize, flags: i32) -> Result<i32, Errno> {
    unsafe { syscall(SYS_SHMGET, key as u64, size as u64, flags as u64, 0, 0, 0) }
        .map(|id| id as i32)
}

fn unique_shmat(shmid: i32, shmaddr: Option<usize>, flags: i32) -> Result<*mut u8, Errno> {
    unsafe {
        syscall(
            SYS_SHMAT,
            shmid as u64,
            shmaddr.map_or(0, |addr| addr as u64),
            flags as u64,
            0,
            0,
            0,
        )
    }
    .map(|addr| addr as *mut u8)
}

fn unique_shmdt(addr: *mut u8) -> Result<(), Errno> {
    unsafe { syscall(SYS_SHMDT, addr as u64, 0, 0, 0, 0, 0) }.map(|_| ())
}

fn unique_shmctl(shmid: i32, cmd: i32, buf: u64) -> Result<i32, Errno> {
    unsafe { syscall(SYS_SHMCTL, shmid as u64, cmd as u64, buf, 0, 0, 0) }.map(|ret| ret as i32)
}

fn unique_shmctl_nobuf(shmid: i32, cmd: i32) -> Result<i32, Errno> {
    unique_shmctl(shmid, cmd, 0)
}

fn unique_shmctl_buf<T>(shmid: i32, cmd: i32, buf: &mut T) -> Result<i32, Errno> {
    unique_shmctl(shmid, cmd, buf as *mut T as u64)
}

fn setuid(uid: u32) -> Result<(), Errno> {
    unsafe { syscall(SYS_SETUID, uid as u64, 0, 0, 0, 0, 0) }.map(|_| ())
}

fn shm_info() -> Result<Shm_Info, Errno> {
    let mut info: Shm_Info = unsafe { zeroed() };
    unique_shmctl_buf(0, SHM_INFO, &mut info)?;
    Ok(info)
}

fn ipc_info() -> Result<ShmInfo, Errno> {
    let mut info: ShmInfo = unsafe { zeroed() };
    unique_shmctl_buf(0, IPC_INFO, &mut info)?;
    Ok(info)
}

fn ipc_stat(shmid: i32) -> Result<ShmIdDs, Errno> {
    let mut ds: ShmIdDs = unsafe { zeroed() };
    unique_shmctl_buf(shmid, IPC_STAT, &mut ds)?;
    Ok(ds)
}

fn shm_stat(index: i32) -> Result<(i32, ShmIdDs), Errno> {
    let mut ds: ShmIdDs = unsafe { zeroed() };
    let id = unique_shmctl_buf(index, SHM_STAT, &mut ds)?;
    Ok((id, ds))
}

fn shm_stat_any(index: i32) -> Result<(i32, ShmIdDs), Errno> {
    let mut ds: ShmIdDs = unsafe { zeroed() };
    let id = unique_shmctl_buf(index, SHM_STAT_ANY, &mut ds)?;
    Ok((id, ds))
}

fn map_anon(addr: usize, length: usize) -> Result<*mut u8, Errno> {
    mmap(
        addr as u64,
        length,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )
    .map(|ptr| ptr.as_ptr())
}

fn load_byte(addr: *mut u8, offset: usize) -> u8 {
    unsafe { read_volatile(addr.add(offset)) }
}

fn store_byte(addr: *mut u8, offset: usize, value: u8) {
    unsafe { write_volatile(addr.add(offset), value) }
}

fn load_flag(addr: *mut u8, offset: usize) -> u8 {
    unsafe { (&*addr.add(offset).cast::<AtomicU8>()).load(Ordering::Acquire) }
}

fn store_flag(addr: *mut u8, offset: usize, value: u8) {
    unsafe { (&*addr.add(offset).cast::<AtomicU8>()).store(value, Ordering::Release) }
}

#[track_caller]
fn wait_for_flag(addr: *mut u8, offset: usize, expected: u8, what: &str) -> Result<(), Errno> {
    for _ in 0..WAIT_RETRIES {
        if load_flag(addr, offset) == expected {
            return Ok(());
        }
        sched_yield()?;
    }
    panic!("{what}: timed out waiting for shared flag at offset {offset} to become {expected}");
}

#[track_caller]
fn expect_errno<T>(result: Result<T, Errno>, expected: Errno, what: &str) {
    match result {
        Ok(_) => panic!("{what}: expected errno {expected}, got success"),
        Err(errno) => assert_eq!(errno, expected, "{what}: unexpected errno"),
    }
}

fn wait_child_status(pid: u32, name: &str) -> Result<WStatus, Errno> {
    loop {
        let mut wstatus = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut wstatus),
            WaitOptions::empty(),
        ) {
            Ok(Some(waited)) => {
                assert_eq!(waited, pid, "shm-test: {name} waited pid mismatch");
                return Ok(wstatus.read());
            },
            Ok(None) => panic!("shm-test: {name} wait4 returned None without WNOHANG"),
            Err(EINTR) => continue,
            Err(errno) => panic!("shm-test: {name} wait4 failed: {errno:?}"),
        }
    }
}

fn wait_child_exit_ok(pid: u32, name: &str) -> Result<(), Errno> {
    match wait_child_status(pid, name)? {
        WStatus::Exited(0) => Ok(()),
        other => panic!("shm-test: {name} child exited unexpectedly: {other:?}"),
    }
}

fn wait_child_signal(pid: u32, expected: i8, name: &str) -> Result<(), Errno> {
    match wait_child_status(pid, name)? {
        WStatus::Signal(sig) if sig == expected => Ok(()),
        other => panic!("shm-test: {name} child did not die as expected: {other:?}"),
    }
}

fn run_test(name: &str, test: TestFn) -> Result<(), Errno> {
    println!("shm-test: CASE {name} start");
    test()?;
    println!("shm-test: CASE {name} ok");
    Ok(())
}

fn test_keyed_lookup_and_rmid() -> Result<(), Errno> {
    let key = test_key(0x01);
    let shmid = unique_shmget(key, PAGE_SIZE, IPC_CREAT | IPC_EXCL | 0o600)?;

    let same = unique_shmget(key, 0, 0)?;
    assert_eq!(
        same, shmid,
        "keyed lookup with size 0 must return the segment"
    );

    expect_errno(
        unique_shmget(key, PAGE_SIZE, IPC_CREAT | IPC_EXCL | 0o600),
        EEXIST,
        "keyed shmget with IPC_EXCL",
    );
    expect_errno(
        unique_shmget(key + 1, PAGE_SIZE, 0),
        ENOENT,
        "missing keyed shmget without IPC_CREAT",
    );
    expect_errno(
        unique_shmget(IPC_PRIVATE, 0, IPC_CREAT | 0o600),
        EINVAL,
        "IPC_PRIVATE zero-sized create",
    );
    expect_errno(
        unique_shmget(key, PAGE_SIZE * 2, 0),
        EINVAL,
        "existing keyed shmget with oversized request",
    );

    let addr = unique_shmat(shmid, None, 0)?;
    unique_shmctl_nobuf(shmid, IPC_RMID)?;
    expect_errno(
        unique_shmat(shmid, None, 0),
        EIDRM,
        "attach after IPC_RMID must fail",
    );
    expect_errno(
        unique_shmget(key, PAGE_SIZE, 0),
        ENOENT,
        "removed key must no longer resolve",
    );
    unique_shmdt(addr)?;

    Ok(())
}

fn test_metadata_stats_and_ctl() -> Result<(), Errno> {
    let baseline_info = shm_info()?;
    let baseline_ipc = ipc_info()?;
    let pid = process_id() as i32;

    let shmid = unique_shmget(IPC_PRIVATE, PAGE_SIZE * 2, IPC_CREAT | 0o600)?;
    let mut ds = ipc_stat(shmid)?;

    assert_eq!(ds.shm_perm.key, IPC_PRIVATE);
    assert_eq!(ds.shm_perm.uid, 0);
    assert_eq!(ds.shm_perm.gid, 0);
    assert_eq!(ds.shm_perm.cuid, 0);
    assert_eq!(ds.shm_perm.cgid, 0);
    assert_eq!(ds.shm_perm.mode & 0o777, 0o600);
    assert_eq!(ds.shm_segsz, (PAGE_SIZE * 2) as u64);
    assert_eq!(ds.shm_nattch, 0);
    assert_eq!(ds.shm_atime, 0);
    assert_eq!(ds.shm_dtime, 0);
    assert_eq!(ds.shm_cpid, pid);

    let after_create = shm_info()?;
    assert_eq!(after_create.used_ids, baseline_info.used_ids + 1);
    assert_eq!(after_create.shm_tot, baseline_info.shm_tot + 2);
    assert_eq!(after_create.shm_rss, baseline_info.shm_rss);
    assert_eq!(after_create.shm_swp, 0);

    let addr = unique_shmat(shmid, None, 0)?;
    assert_eq!(
        addr as usize % PAGE_SIZE,
        0,
        "shmat(NULL) must return a page-aligned address",
    );

    store_byte(addr, 0, 0x7a);
    store_byte(addr, PAGE_SIZE, 0x3c);

    ds = ipc_stat(shmid)?;
    assert_eq!(ds.shm_nattch, 1);
    assert_eq!(ds.shm_dtime, 0);
    assert_eq!(ds.shm_lpid, pid);

    let after_touch = shm_info()?;
    assert_eq!(after_touch.shm_rss, baseline_info.shm_rss + 2);
    assert_eq!(after_touch.shm_tot, baseline_info.shm_tot + 2);

    let index = shmid & 0xffff;
    let (stat_id, stat_ds) = shm_stat(index)?;
    assert_eq!(stat_id, shmid);
    assert_eq!(stat_ds.shm_segsz, ds.shm_segsz);
    assert_eq!(stat_ds.shm_nattch, ds.shm_nattch);

    let (stat_any_id, stat_any_ds) = shm_stat_any(index)?;
    assert_eq!(stat_any_id, shmid);
    assert_eq!(stat_any_ds.shm_segsz, ds.shm_segsz);
    assert_eq!(stat_any_ds.shm_nattch, ds.shm_nattch);

    let mut new_ds = ds;
    new_ds.shm_perm.uid = 17;
    new_ds.shm_perm.gid = 29;
    new_ds.shm_perm.mode = (new_ds.shm_perm.mode & !0o777) | 0o644;
    unique_shmctl_buf(shmid, IPC_SET, &mut new_ds)?;

    let after_set = ipc_stat(shmid)?;
    assert_eq!(after_set.shm_perm.uid, 17);
    assert_eq!(after_set.shm_perm.gid, 29);
    assert_eq!(after_set.shm_perm.mode & 0o777, 0o644);
    assert!(after_set.shm_ctime >= ds.shm_ctime);

    unique_shmctl_nobuf(shmid, SHM_LOCK)?;
    let after_lock = ipc_stat(shmid)?;
    assert_ne!(after_lock.shm_perm.mode & SHM_LOCKED, 0);
    unique_shmctl_nobuf(shmid, SHM_UNLOCK)?;
    let after_unlock = ipc_stat(shmid)?;
    assert_eq!(after_unlock.shm_perm.mode & SHM_LOCKED, 0);

    unique_shmdt(addr)?;

    let after_detach = ipc_stat(shmid)?;
    assert_eq!(after_detach.shm_nattch, 0);
    assert_eq!(after_detach.shm_lpid, pid);

    unique_shmctl_nobuf(shmid, IPC_RMID)?;

    let final_info = shm_info()?;
    assert_eq!(final_info.used_ids, baseline_info.used_ids);
    assert_eq!(final_info.shm_tot, baseline_info.shm_tot);
    assert_eq!(final_info.shm_rss, baseline_info.shm_rss);

    assert_eq!(baseline_ipc.shmmin, 1);
    assert!(baseline_ipc.shmmax >= PAGE_SIZE as u64);
    assert!(baseline_ipc.shmmni > 0);
    assert_eq!(baseline_ipc.shmseg, baseline_ipc.shmmni);

    Ok(())
}

fn test_fork_shared_and_detach() -> Result<(), Errno> {
    const CHILD_READY: usize = 0;
    const PARENT_RELEASE: usize = 1;
    const CHILD_WRITE: usize = 2;

    let shmid = unique_shmget(IPC_PRIVATE, PAGE_SIZE, IPC_CREAT | 0o600)?;
    let addr = unique_shmat(shmid, None, 0)?;
    store_byte(addr, 0, 11);

    let pid = match fork()? {
        Some(pid) => pid,
        None => {
            assert_eq!(load_byte(addr, 0), 11);
            store_flag(addr, CHILD_READY, 1);
            wait_for_flag(addr, PARENT_RELEASE, 1, "child waiting for parent release")?;
            store_byte(addr, CHILD_WRITE, 77);
            exit(0);
        },
    };

    wait_for_flag(addr, CHILD_READY, 1, "parent waiting for child ready")?;

    let while_child = ipc_stat(shmid)?;
    assert_eq!(while_child.shm_nattch, 2);

    store_flag(addr, PARENT_RELEASE, 1);
    wait_child_exit_ok(pid, "fork-shared-and-detach")?;

    assert_eq!(load_byte(addr, CHILD_WRITE), 77);

    let after_child_exit = ipc_stat(shmid)?;
    assert_eq!(after_child_exit.shm_nattch, 1);
    assert_eq!(after_child_exit.shm_lpid, pid as i32);

    unique_shmdt(addr)?;

    let after_detach = ipc_stat(shmid)?;
    assert_eq!(after_detach.shm_nattch, 0);

    unique_shmctl_nobuf(shmid, IPC_RMID)?;
    Ok(())
}

fn test_rounding_and_remap() -> Result<(), Errno> {
    let shmid = unique_shmget(IPC_PRIVATE, PAGE_SIZE, IPC_CREAT | 0o600)?;

    let base = unique_shmat(shmid, None, 0)?;
    unique_shmdt(base)?;

    let hinted = (base as usize) + PAGE_SIZE / 2;
    let rounded = unique_shmat(shmid, Some(hinted), SHM_RND)?;
    assert_eq!(rounded, base);
    unique_shmdt(rounded)?;

    let second = unique_shmget(IPC_PRIVATE, PAGE_SIZE, IPC_CREAT | 0o600)?;
    let shm_addr = unique_shmat(second, None, 0)?;
    unique_shmdt(shm_addr)?;

    let anon = map_anon(0, PAGE_SIZE)?;
    store_byte(anon, 0, 0xaa);
    store_byte(anon, PAGE_SIZE - 1, 0xbb);

    let remapped = unique_shmat(second, Some(anon as usize), SHM_REMAP)?;
    assert_eq!(remapped, anon);
    assert_eq!(load_byte(remapped, 0), 0);
    assert_eq!(load_byte(remapped, PAGE_SIZE - 1), 0);

    store_byte(remapped, 0, 0x44);
    store_byte(remapped, PAGE_SIZE - 1, 0x55);
    assert_eq!(load_byte(remapped, 0), 0x44);
    assert_eq!(load_byte(remapped, PAGE_SIZE - 1), 0x55);

    unique_shmdt(remapped)?;
    unique_shmctl_nobuf(shmid, IPC_RMID)?;
    unique_shmctl_nobuf(second, IPC_RMID)?;
    Ok(())
}

fn test_readonly_and_detach_fault() -> Result<(), Errno> {
    let readonly_id = unique_shmget(IPC_PRIVATE, PAGE_SIZE, IPC_CREAT | 0o600)?;
    let readonly_pid = match fork()? {
        Some(pid) => pid,
        None => {
            let addr = unique_shmat(readonly_id, None, SHM_RDONLY)?;
            assert_eq!(load_byte(addr, 0), 0);
            store_byte(addr, 0, 1);
            exit(0);
        },
    };
    wait_child_signal(readonly_pid, SIGSEGV as i8, "readonly write fault")?;
    unique_shmctl_nobuf(readonly_id, IPC_RMID)?;

    let detach_id = unique_shmget(IPC_PRIVATE, PAGE_SIZE, IPC_CREAT | 0o600)?;
    let detach_pid = match fork()? {
        Some(pid) => pid,
        None => {
            let addr = unique_shmat(detach_id, None, 0)?;
            store_byte(addr, 0, 2);
            unique_shmdt(addr)?;
            store_byte(addr, 0, 3);
            exit(0);
        },
    };
    wait_child_signal(detach_pid, SIGSEGV as i8, "detach fault")?;
    unique_shmctl_nobuf(detach_id, IPC_RMID)?;
    Ok(())
}

fn test_credential_permissions() -> Result<(), Errno> {
    let key = test_key(0x07);
    let shmid = unique_shmget(key, PAGE_SIZE, IPC_CREAT | IPC_EXCL | 0o600)?;
    let index = shmid & 0xffff;

    let pid = match fork()? {
        Some(pid) => pid,
        None => {
            setuid(65534)?;

            expect_errno(
                unique_shmget(key, PAGE_SIZE, SHM_R),
                EACCES,
                "unprivileged keyed shmget read access",
            );
            expect_errno(
                unique_shmat(shmid, None, 0),
                EACCES,
                "unprivileged shmat read-write access",
            );

            let mut ds: ShmIdDs = unsafe { zeroed() };
            expect_errno(
                unique_shmctl_buf(shmid, IPC_STAT, &mut ds),
                EACCES,
                "unprivileged IPC_STAT",
            );
            expect_errno(
                unique_shmctl_buf(shmid, IPC_SET, &mut ds),
                EPERM,
                "unprivileged IPC_SET",
            );
            expect_errno(
                unique_shmctl_nobuf(shmid, IPC_RMID),
                EPERM,
                "unprivileged IPC_RMID",
            );
            expect_errno(
                unique_shmctl_nobuf(shmid, SHM_LOCK),
                EPERM,
                "unprivileged SHM_LOCK",
            );
            expect_errno(
                unique_shmctl_nobuf(shmid, SHM_UNLOCK),
                EPERM,
                "unprivileged SHM_UNLOCK",
            );

            let (stat_any_id, stat_any_ds) = shm_stat_any(index)?;
            assert_eq!(stat_any_id, shmid);
            assert_eq!(stat_any_ds.shm_perm.mode & 0o777, 0o600);

            exit(0);
        },
    };

    wait_child_exit_ok(pid, "credential-permissions")?;
    unique_shmctl_nobuf(shmid, IPC_RMID)?;
    Ok(())
}

fn test_invalid_arguments() -> Result<(), Errno> {
    let shmid = unique_shmget(IPC_PRIVATE, PAGE_SIZE, IPC_CREAT | 0o600)?;

    expect_errno(
        unique_shmget(IPC_PRIVATE, 0, 0o600),
        EINVAL,
        "zero-sized IPC_PRIVATE shmget",
    );
    expect_errno(
        unique_shmat(shmid, None, 0x4000_0000),
        EINVAL,
        "unsupported shmat flags",
    );
    expect_errno(
        unique_shmat(shmid, Some(PAGE_SIZE + 1), 0),
        EINVAL,
        "unaligned shmat address",
    );
    expect_errno(
        unique_shmat(shmid, None, SHM_REMAP),
        EINVAL,
        "SHM_REMAP without an address",
    );

    let addr = unique_shmat(shmid, None, 0)?;
    expect_errno(
        unique_shmdt(unsafe { addr.add(1) }),
        EINVAL,
        "unaligned shmdt address",
    );
    unique_shmdt(addr)?;
    expect_errno(
        unique_shmdt(addr),
        EINVAL,
        "double shmdt of the same address",
    );

    unique_shmctl_nobuf(shmid, IPC_RMID)?;
    Ok(())
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    println!("===== shm test =====");

    for (name, test) in TESTS {
        run_test(name, *test)?;
    }

    println!("shm-test: all cases passed");
    Ok(())
}
