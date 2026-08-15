use core::mem::size_of;

use anemone_rs::{
    abi::{
        fs::linux::open::{O_RDONLY, O_RDWR},
        process::linux::{
            clone::CloneArgs,
            sched::{CPU_SETSIZE, CpuSet},
            signal::SIGCHLD,
        },
        syscall::{
            linux::{SYS_CLONE3, SYS_SCHED_GETAFFINITY, SYS_SCHED_SETAFFINITY},
            syscall,
        },
    },
    os::{
        anemone::nemophila::{load_embedded, load_supplied, try_unload},
        linux::{
            fs::{AtFd, PipeFlags, close, ftruncate, pipe2, read, renameat2, unlinkat, write},
            process::{WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork, wait4},
        },
    },
    prelude::*,
};

use crate::{
    abi::{ARTIFACT, MUTABLE_ARTIFACT, OwnedFd, open, raw_try_unload},
    procfs::{self, DirectorySnapshot},
};

fn ensure(condition: bool) -> Result<(), Errno> {
    if condition { Ok(()) } else { Err(EIO) }
}

fn wait_child_exit(child: u32) -> Result<i8, Errno> {
    let mut status = WStatusRaw::EMPTY;
    ensure(
        wait4(
            WaitFor::ChildWithTgid(child),
            Some(&mut status),
            WaitOptions::empty(),
        )? == Some(child),
    )?;
    match status.read() {
        WStatus::Exited(code) => Ok(code),
        _ => Err(EIO),
    }
}

fn wait_child(child: u32) -> Result<(), Errno> {
    ensure(wait_child_exit(child)? == 0)
}

fn set_current_affinity(mask: &CpuSet) -> Result<(), Errno> {
    unsafe {
        syscall(
            SYS_SCHED_SETAFFINITY,
            0,
            size_of::<CpuSet>() as u64,
            mask as *const CpuSet as u64,
            0,
            0,
            0,
        )
    }
    .map(|_| ())
}

fn pin_current_to(cpu: usize) -> Result<(), Errno> {
    let mut mask = CpuSet::empty();
    mask.set(cpu);
    set_current_affinity(&mask)
}

fn current_affinity() -> Result<CpuSet, Errno> {
    let mut mask = CpuSet::empty();
    let copied = unsafe {
        syscall(
            SYS_SCHED_GETAFFINITY,
            0,
            size_of::<CpuSet>() as u64,
            &mut mask as *mut CpuSet as u64,
            0,
            0,
            0,
        )
    }?;
    ensure(copied as usize == size_of::<usize>())?;
    Ok(mask)
}

fn helper_cpu() -> Result<usize, Errno> {
    let available = current_affinity()?;
    let mut owner = None;
    for cpu in 0..CPU_SETSIZE {
        if !available.contains(cpu) {
            continue;
        }
        match pin_current_to(cpu) {
            Ok(()) => {
                owner = Some(cpu);
                break;
            },
            Err(EINVAL) => {},
            Err(error) => return Err(error),
        }
    }
    set_current_affinity(&available)?;
    let owner = owner.ok_or(EIO)?;
    (0..CPU_SETSIZE)
        .find(|cpu| *cpu != owner && available.contains(*cpu))
        .ok_or(EIO)
}

fn fork_once() -> Result<(), Errno> {
    match fork()? {
        Some(child) => wait_child(child),
        None => exit(0),
    }
}

fn raw_clone3() -> Result<u32, Errno> {
    let args = CloneArgs {
        flags: 0,
        pidfd: 0,
        child_tid: 0,
        parent_tid: 0,
        exit_signal: SIGCHLD as u64,
        stack: 0,
        stack_size: 0,
        tls: 0,
        set_tid: 0,
        set_tid_size: 0,
        cgroup: 0,
    };
    unsafe {
        syscall(
            SYS_CLONE3,
            &args as *const CloneArgs as u64,
            size_of::<CloneArgs>() as u64,
            0,
            0,
            0,
            0,
        )
    }
    .map(|tid| tid as u32)
}

fn clone3_once() -> Result<(), Errno> {
    let child = raw_clone3()?;
    if child == 0 {
        exit(0);
    }
    wait_child(child)
}

fn read_exact(fd: u32, mut bytes: &mut [u8]) -> Result<(), Errno> {
    while !bytes.is_empty() {
        let count = read(fd, bytes)?;
        if count == 0 {
            return Err(EIO);
        }
        bytes = &mut bytes[count..];
    }
    Ok(())
}

fn write_all(fd: u32, mut bytes: &[u8]) -> Result<(), Errno> {
    while !bytes.is_empty() {
        let count = write(fd, bytes)?;
        if count == 0 {
            return Err(EIO);
        }
        bytes = &bytes[count..];
    }
    Ok(())
}

struct BusyHelper {
    child: u32,
    command: OwnedFd,
    result: OwnedFd,
}

impl BusyHelper {
    fn start(helper_cpu: usize) -> Result<Self, Errno> {
        // Tasks have immutable scheduler owners. Select a child already owned
        // by the other online CPU instead of pretending sched_setaffinity can
        // migrate it. The BSP's logical core differs between RV64 and LA64.
        for _ in 0..4 {
            let (command_read, command_write) = pipe2(PipeFlags::empty())?;
            let (result_read, result_write) = pipe2(PipeFlags::empty())?;
            match fork()? {
                Some(child) => {
                    close(command_read)?;
                    close(result_write)?;
                    let candidate = Self {
                        child,
                        command: OwnedFd(command_write),
                        result: OwnedFd(result_read),
                    };
                    let mut ready = [0u8; 1];
                    read_exact(candidate.result.0, &mut ready)?;
                    if ready[0] == 1 {
                        return Ok(candidate);
                    }
                    ensure(ready[0] == 0)?;
                    let child = candidate.child;
                    drop(candidate);
                    ensure(wait_child_exit(child)? == 2)?;
                },
                None => {
                    let _ = close(command_write);
                    let _ = close(result_read);
                    match pin_current_to(helper_cpu) {
                        Ok(()) => {
                            let _ = write_all(result_write, &[1]);
                            busy_helper(command_read, result_write)
                        },
                        Err(EINVAL) => {
                            let _ = write_all(result_write, &[0]);
                            let _ = close(command_read);
                            let _ = close(result_write);
                            exit(2)
                        },
                        Err(_) => exit(3),
                    }
                },
            }
        }
        Err(EIO)
    }

    fn arm(&self, identity: u64) -> Result<(), Errno> {
        write_all(self.command.0, &identity.to_ne_bytes())?;
        let mut ready = [0u8; 1];
        read_exact(self.result.0, &mut ready)?;
        ensure(ready[0] == 1)
    }

    fn finish(self) -> Result<(), Errno> {
        let mut result = [0u8; 1];
        read_exact(self.result.0, &mut result)?;
        ensure(result[0] == 1)?;
        wait_child(self.child)
    }
}

fn busy_helper(command: u32, result: u32) -> ! {
    let mut identity = [0u8; 8];
    let passed = read_exact(command, &mut identity)
        .and_then(|()| {
            let identity = u64::from_ne_bytes(identity);
            write_all(result, &[1])?;
            procfs::wait_in_flight(identity)?;
            ensure(raw_try_unload(identity, 0) == Err(EBUSY))
        })
        .is_ok();
    let _ = write_all(result, &[u8::from(passed)]);
    let _ = close(command);
    let _ = close(result);
    exit(if passed { 0 } else { 1 })
}

fn initial_boot_instance(helper: &BusyHelper) -> Result<(DirectorySnapshot, u64), Errno> {
    // The authorization child was the canonical artifact's first normal
    // callback. Creating this helper is its intentional second-callback trap.
    let identities = DirectorySnapshot::open()?.identities()?;
    ensure(identities.len() == 1)?;
    let boot = identities[0];
    procfs::assert_fields(boot, "embedded", "clone-observer", "poisoned")?;
    let retained = procfs::open_instance(boot)?;
    try_unload(boot)?;
    ensure(procfs::open_instance(boot).err() == Some(ENOENT))?;
    let retained_text = procfs::read_all(&retained)?;
    ensure(retained_text.contains("lifecycle: poisoned\n"))?;
    let empty = DirectorySnapshot::open()?;
    ensure(empty.identities()?.is_empty())?;
    let _ = helper;
    Ok((empty, boot))
}

fn load_sources(boot: u64) -> Result<(u64, u64, u64), Errno> {
    let embedded = load_embedded("clone-observer")?;

    let supplied_file = open(ARTIFACT, O_RDONLY)?;
    let mut byte = [0u8; 1];
    read_exact(supplied_file.0, &mut byte)?;
    ensure(byte[0] == 0)?;
    let supplied = load_supplied(supplied_file.0 as i32)?;
    read_exact(supplied_file.0, &mut byte)?;
    ensure(byte[0] == b'a')?;

    let mutable_file = open(MUTABLE_ARTIFACT, O_RDWR)?;
    let mutable = load_supplied(mutable_file.0 as i32)?;
    ftruncate(mutable_file.0, 0)?;
    drop(mutable_file);
    renameat2(
        AtFd::Cwd,
        Path::new(MUTABLE_ARTIFACT),
        AtFd::Cwd,
        Path::new("/modules/clone-observer-retired-source.wasm"),
        0,
    )?;
    unlinkat(
        AtFd::Cwd,
        Path::new("/modules/clone-observer-retired-source.wasm"),
        0,
    )?;

    ensure(embedded > boot && supplied > embedded && mutable > supplied)?;
    Ok((embedded, supplied, mutable))
}

fn validate_proc(
    empty: DirectorySnapshot,
    embedded: u64,
    supplied: u64,
    mutable: u64,
) -> Result<OwnedFd, Errno> {
    ensure(empty.identities()?.is_empty())?;
    let identities = DirectorySnapshot::open()?.identities()?;
    ensure(identities == [embedded, supplied, mutable])?;
    procfs::assert_fields(embedded, "embedded", "clone-observer", "live")?;
    procfs::assert_fields(supplied, "supplied", "-", "live")?;
    procfs::assert_fields(mutable, "supplied", "-", "live")?;
    let file = procfs::open_instance(embedded)?;
    ensure(write(file.0, b"x").is_err())?;
    ensure(procfs::open_instance(u64::MAX).err() == Some(ENOENT))?;
    Ok(file)
}

fn poison_and_retire(
    helper: BusyHelper,
    retained: OwnedFd,
    embedded: u64,
    supplied: u64,
    mutable: u64,
) -> Result<(), Errno> {
    // Every fresh instance returns normally once, proving real fanout.
    fork_once()?;
    helper.arm(embedded)?;
    // The second invocation is raw clone3. Each instance remains in-flight
    // through its bounded trap window while the helper proves EBUSY.
    clone3_once()?;
    helper.finish()?;

    for identity in [embedded, supplied, mutable] {
        procfs::assert_fields(
            identity,
            if identity == embedded {
                "embedded"
            } else {
                "supplied"
            },
            if identity == embedded {
                "clone-observer"
            } else {
                "-"
            },
            "poisoned",
        )?;
        try_unload(identity)?;
        ensure(try_unload(identity) == Err(ENOENT))?;
    }
    ensure(procfs::read_all(&retained)?.contains("lifecycle: live\n"))?;
    ensure(procfs::open_instance(embedded).err() == Some(ENOENT))?;

    let reloaded = load_embedded("clone-observer")?;
    ensure(reloaded > mutable)?;
    procfs::assert_fields(reloaded, "embedded", "clone-observer", "live")?;
    fork_once()?;
    try_unload(reloaded)?;
    ensure(raw_try_unload(reloaded, 0) == Err(ENOENT))?;
    Ok(())
}

pub(crate) fn run() -> Result<(), Errno> {
    println!("NEMOPHILA-R0:CASE:LIFECYCLE:START");
    let helper = BusyHelper::start(helper_cpu()?)?;
    let (empty, boot) = initial_boot_instance(&helper)?;
    let (embedded, supplied, mutable) = load_sources(boot)?;
    let retained = validate_proc(empty, embedded, supplied, mutable)?;
    poison_and_retire(helper, retained, embedded, supplied, mutable)?;
    println!("NEMOPHILA-R0:CASE:LIFECYCLE:PASS");
    Ok(())
}
