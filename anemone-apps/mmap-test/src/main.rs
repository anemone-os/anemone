#![no_std]
#![no_main]

use core::{
    ptr::{null_mut, read_volatile, write_volatile},
    slice,
    sync::atomic::{AtomicU8, Ordering},
};

use anemone_rs::{
    os::linux::process::{
        self, CloneFlags, MmapFlags, MmapProt, WStatus, WStatusRaw, WaitOptions, clone, getpid,
        mmap, mprotect, munmap, wait4,
    },
    prelude::*,
};

const PAGE_SIZE: usize = 4096;
const WAIT_RETRIES: usize = 1_000_000;

type TestFn = fn() -> Result<(), Errno>;

const TESTS: &[(&str, TestFn)] = &[
    ("basic-anon-private", test_basic_anonymous_private_mapping),
    ("invalid-arguments", test_invalid_arguments),
    ("munmap-hole-and-mprotect", test_partial_unmap_and_mprotect),
    (
        "fixed-replace-and-noreplace",
        test_fixed_replace_and_noreplace,
    ),
    (
        "fork-shared-vs-private",
        test_fork_shared_and_private_behavior,
    ),
    (
        "multi-fork-sequential-snapshots",
        test_multi_fork_sequential_snapshots,
    ),
    ("multi-fork-nested-lineage", test_multi_fork_nested_lineage),
];

fn map_anon(addr: u64, length: usize, prot: MmapProt, flags: MmapFlags) -> Result<*mut u8, Errno> {
    mmap(addr, length, prot, flags, None, None).map(|ptr| ptr.as_ptr())
}

unsafe fn bytes_mut<'a>(addr: *mut u8, length: usize) -> &'a mut [u8] {
    unsafe { slice::from_raw_parts_mut(addr, length) }
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
        process::sched_yield()?;
    }
    panic!("{what}: timed out waiting for shared flag at offset {offset} to become {expected}");
}

#[track_caller]
fn expect_err<T>(result: Result<T, Errno>, what: &str) -> Errno {
    match result {
        Ok(_) => panic!("{what}: expected errno, got success"),
        Err(errno) => errno,
    }
}

#[track_caller]
fn expect_errno<T>(result: Result<T, Errno>, expected: Errno, what: &str) {
    match result {
        Ok(_) => panic!("{what}: expected errno {expected}, got success"),
        Err(errno) => assert_eq!(errno, expected, "{what}: unexpected errno"),
    }
}

fn fork_like() -> Result<u32, Errno> {
    let mut child_tid = 0;
    clone(
        CloneFlags::CLONE_CHILD_SETTID,
        None,
        None,
        null_mut(),
        Some(&mut child_tid),
    )
}

fn wait_child_exit_ok(pid: u32, name: &str) -> Result<(), Errno> {
    let mut wstatus = WStatusRaw::EMPTY;
    match wait4(pid as i64, Some(&mut wstatus), WaitOptions::empty())? {
        Some(waited) => {
            assert_eq!(waited, pid, "{name}: waited pid mismatch");
            match wstatus.read() {
                WStatus::Exited(0) => Ok(()),
                other => panic!("{name}: child exited unexpectedly: {other:?}"),
            }
        },
        None => panic!("{name}: wait4 returned None without WNOHANG"),
    }
}

fn run_test(name: &str, test: TestFn) -> Result<(), Errno> {
    println!("mmap-test: CASE {name} start");
    test()?;
    println!("mmap-test: CASE {name} ok");
    Ok(())
}

fn test_basic_anonymous_private_mapping() -> Result<(), Errno> {
    let length = PAGE_SIZE * 3;
    let base = map_anon(
        0,
        length,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
    )?;

    assert_eq!(
        base as usize % PAGE_SIZE,
        0,
        "mmap result must be page aligned"
    );
    assert_eq!(load_byte(base, 0), 0);
    assert_eq!(load_byte(base, PAGE_SIZE), 0);
    assert_eq!(load_byte(base, length - 1), 0);

    store_byte(base, 0, 0x11);
    store_byte(base, PAGE_SIZE - 1, 0x22);
    store_byte(base, PAGE_SIZE, 0x33);
    store_byte(base, PAGE_SIZE * 2 + 17, 0x44);
    store_byte(base, length - 1, 0x55);

    assert_eq!(load_byte(base, 0), 0x11);
    assert_eq!(load_byte(base, PAGE_SIZE - 1), 0x22);
    assert_eq!(load_byte(base, PAGE_SIZE), 0x33);
    assert_eq!(load_byte(base, PAGE_SIZE * 2 + 17), 0x44);
    assert_eq!(load_byte(base, length - 1), 0x55);

    munmap(base, length)
}

fn test_invalid_arguments() -> Result<(), Errno> {
    let base = map_anon(
        0,
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
    )?;

    expect_err(
        map_anon(
            0,
            0,
            MmapProt::PROT_READ | MmapProt::PROT_WRITE,
            MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        ),
        "zero-length mmap",
    );
    expect_err(
        mmap(
            0,
            PAGE_SIZE,
            MmapProt::PROT_READ,
            MmapFlags::MAP_ANONYMOUS,
            None,
            None,
        ),
        "mmap without MAP_PRIVATE/MAP_SHARED",
    );
    expect_err(
        mmap(
            0,
            PAGE_SIZE,
            MmapProt::PROT_READ | MmapProt::PROT_WRITE,
            MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
            None,
            Some(PAGE_SIZE),
        ),
        "anonymous mmap with non-zero offset",
    );
    expect_err(
        map_anon(
            base as u64 + 1,
            PAGE_SIZE,
            MmapProt::PROT_READ,
            MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS | MmapFlags::MAP_FIXED,
        ),
        "unaligned MAP_FIXED address",
    );
    expect_err(
        map_anon(
            base as u64,
            PAGE_SIZE,
            MmapProt::PROT_READ,
            MmapFlags::MAP_PRIVATE
                | MmapFlags::MAP_ANONYMOUS
                | MmapFlags::MAP_FIXED
                | MmapFlags::MAP_FIXED_NOREPLACE,
        ),
        "MAP_FIXED and MAP_FIXED_NOREPLACE together",
    );
    expect_err(
        munmap(unsafe { base.add(1) }, PAGE_SIZE),
        "unaligned munmap",
    );
    expect_err(munmap(base, 0), "zero-length munmap");
    expect_err(
        mprotect(unsafe { base.add(1) }, PAGE_SIZE, MmapProt::PROT_READ),
        "unaligned mprotect",
    );
    expect_err(
        mprotect(base, 0, MmapProt::PROT_READ),
        "zero-length mprotect",
    );
    expect_err(
        mprotect(
            unsafe { base.add(PAGE_SIZE) },
            PAGE_SIZE,
            MmapProt::PROT_READ,
        ),
        "mprotect on unmapped range",
    );

    munmap(base, PAGE_SIZE)
}

fn test_partial_unmap_and_mprotect() -> Result<(), Errno> {
    let length = PAGE_SIZE * 5;
    let base = map_anon(
        0,
        length,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
    )?;

    store_byte(base, 0, 0xaa);
    store_byte(base, PAGE_SIZE * 4 + 7, 0xbb);

    munmap(unsafe { base.add(PAGE_SIZE) }, PAGE_SIZE * 2)?;

    expect_errno(
        mprotect(base, length, MmapProt::PROT_NONE),
        ENOMEM,
        "mprotect across a hole",
    );
    expect_errno(
        mprotect(
            unsafe { base.add(PAGE_SIZE) },
            PAGE_SIZE,
            MmapProt::PROT_READ,
        ),
        ENOMEM,
        "mprotect on the punched hole",
    );

    mprotect(base, PAGE_SIZE, MmapProt::PROT_NONE)?;
    mprotect(base, PAGE_SIZE, MmapProt::PROT_READ | MmapProt::PROT_WRITE)?;
    mprotect(
        unsafe { base.add(PAGE_SIZE * 3) },
        PAGE_SIZE * 2,
        MmapProt::PROT_READ,
    )?;

    assert_eq!(load_byte(base, 0), 0xaa);
    assert_eq!(load_byte(base, PAGE_SIZE * 4 + 7), 0xbb);

    munmap(base, PAGE_SIZE)?;
    munmap(unsafe { base.add(PAGE_SIZE * 3) }, PAGE_SIZE * 2)
}

fn test_fixed_replace_and_noreplace() -> Result<(), Errno> {
    let length = PAGE_SIZE * 6;
    let base = map_anon(
        0,
        length,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
    )?;

    for page in 0..6 {
        store_byte(base, page * PAGE_SIZE, 0x10 + page as u8);
        store_byte(base, page * PAGE_SIZE + 31, 0x80 + page as u8);
    }

    expect_errno(
        map_anon(
            base as u64 + (PAGE_SIZE * 2) as u64,
            PAGE_SIZE * 2,
            MmapProt::PROT_READ | MmapProt::PROT_WRITE,
            MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS | MmapFlags::MAP_FIXED_NOREPLACE,
        ),
        EFAULT,
        "MAP_FIXED_NOREPLACE over existing mapping",
    );

    let replaced = map_anon(
        base as u64 + (PAGE_SIZE * 2) as u64,
        PAGE_SIZE * 2,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS | MmapFlags::MAP_FIXED,
    )?;
    assert_eq!(replaced, unsafe { base.add(PAGE_SIZE * 2) });

    assert_eq!(load_byte(base, 0), 0x10);
    assert_eq!(load_byte(base, PAGE_SIZE + 31), 0x81);
    assert_eq!(load_byte(base, PAGE_SIZE * 2), 0);
    assert_eq!(load_byte(base, PAGE_SIZE * 3 + 31), 0);
    assert_eq!(load_byte(base, PAGE_SIZE * 4), 0x14);
    assert_eq!(load_byte(base, PAGE_SIZE * 5 + 31), 0x85);

    store_byte(base, PAGE_SIZE * 2 + 13, 0xcd);
    store_byte(base, PAGE_SIZE * 3 + 29, 0xef);
    assert_eq!(load_byte(base, PAGE_SIZE * 2 + 13), 0xcd);
    assert_eq!(load_byte(base, PAGE_SIZE * 3 + 29), 0xef);

    munmap(base, length)
}

fn test_fork_shared_and_private_behavior() -> Result<(), Errno> {
    const CHILD_PRIVATE_SNAPSHOT: usize = 0;
    const CHILD_READY: usize = 1;
    const PARENT_RELEASE: usize = 2;
    const CHILD_SHARED_WRITE: usize = 3;

    let shared = map_anon(
        0,
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_SHARED | MmapFlags::MAP_ANONYMOUS,
    )?;
    let private = map_anon(
        0,
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
    )?;

    unsafe { bytes_mut(shared, PAGE_SIZE) }.fill(0);
    unsafe { bytes_mut(private, PAGE_SIZE) }.fill(0);
    store_byte(private, 0, 11);

    let pid = fork_like()?;
    if pid == 0 {
        store_byte(shared, CHILD_PRIVATE_SNAPSHOT, load_byte(private, 0));
        store_flag(shared, CHILD_READY, 1);
        wait_for_flag(
            shared,
            PARENT_RELEASE,
            1,
            "child waiting for parent release",
        )
        .expect("child failed while waiting for parent release");

        store_byte(private, 0, 33);
        store_byte(shared, CHILD_SHARED_WRITE, 44);
        process::exit(0);
    }

    store_byte(private, 0, 22);
    wait_for_flag(shared, CHILD_READY, 1, "parent waiting for child snapshot")?;
    assert_eq!(
        load_byte(shared, CHILD_PRIVATE_SNAPSHOT),
        11,
        "child must observe pre-fork private snapshot"
    );
    store_flag(shared, PARENT_RELEASE, 1);
    wait_child_exit_ok(pid, "fork-shared-vs-private")?;

    assert_eq!(
        load_byte(private, 0),
        22,
        "parent must not observe child's private write"
    );
    assert_eq!(
        load_byte(shared, CHILD_SHARED_WRITE),
        44,
        "shared mapping write must propagate across fork"
    );

    munmap(shared, PAGE_SIZE)?;
    munmap(private, PAGE_SIZE)
}

fn test_multi_fork_sequential_snapshots() -> Result<(), Errno> {
    const FIRST_CHILD_PRIVATE_SNAPSHOT: usize = 0;
    const FIRST_CHILD_SHARED_WRITE: usize = 1;
    const SECOND_CHILD_PRIVATE_SNAPSHOT: usize = 2;
    const SECOND_CHILD_SHARED_SNAPSHOT: usize = 3;
    const SECOND_CHILD_SHARED_WRITE: usize = 4;

    let shared = map_anon(
        0,
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_SHARED | MmapFlags::MAP_ANONYMOUS,
    )?;
    let private = map_anon(
        0,
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
    )?;

    unsafe { bytes_mut(shared, PAGE_SIZE) }.fill(0);
    unsafe { bytes_mut(private, PAGE_SIZE) }.fill(0);
    store_byte(private, 0, 10);

    let first_pid = fork_like()?;
    if first_pid == 0 {
        store_byte(shared, FIRST_CHILD_PRIVATE_SNAPSHOT, load_byte(private, 0));
        store_byte(private, 1, 41);
        store_byte(shared, FIRST_CHILD_SHARED_WRITE, 51);
        process::exit(0);
    }

    store_byte(private, 0, 20);
    wait_child_exit_ok(first_pid, "multi-fork-sequential-snapshots/child1")?;

    let second_pid = fork_like()?;
    if second_pid == 0 {
        store_byte(shared, SECOND_CHILD_PRIVATE_SNAPSHOT, load_byte(private, 0));
        store_byte(
            shared,
            SECOND_CHILD_SHARED_SNAPSHOT,
            load_byte(shared, FIRST_CHILD_SHARED_WRITE),
        );
        store_byte(private, 1, 42);
        store_byte(shared, SECOND_CHILD_SHARED_WRITE, 52);
        process::exit(0);
    }

    store_byte(private, 0, 30);
    wait_child_exit_ok(second_pid, "multi-fork-sequential-snapshots/child2")?;

    assert_eq!(
        load_byte(shared, FIRST_CHILD_PRIVATE_SNAPSHOT),
        10,
        "first child must observe the private snapshot from its own fork point"
    );
    assert_eq!(
        load_byte(shared, FIRST_CHILD_SHARED_WRITE),
        51,
        "first child shared write must propagate"
    );
    assert_eq!(
        load_byte(shared, SECOND_CHILD_PRIVATE_SNAPSHOT),
        20,
        "second child must observe parent's later private snapshot"
    );
    assert_eq!(
        load_byte(shared, SECOND_CHILD_SHARED_SNAPSHOT),
        51,
        "second child must inherit shared state from first child"
    );
    assert_eq!(
        load_byte(shared, SECOND_CHILD_SHARED_WRITE),
        52,
        "second child shared write must propagate"
    );
    assert_eq!(
        load_byte(private, 0),
        30,
        "parent private write after second fork must stay local"
    );
    assert_eq!(
        load_byte(private, 1),
        0,
        "parent must not observe private writes from either child"
    );

    munmap(shared, PAGE_SIZE)?;
    munmap(private, PAGE_SIZE)
}

fn test_multi_fork_nested_lineage() -> Result<(), Errno> {
    const CHILD_PRIVATE_SNAPSHOT: usize = 0;
    const GRANDCHILD_PRIVATE_SNAPSHOT: usize = 1;
    const GRANDCHILD_SHARED_WRITE: usize = 2;
    const CHILD_PRIVATE_AFTER_GRANDCHILD: usize = 3;
    const CHILD_SHARED_WRITE: usize = 4;

    let shared = map_anon(
        0,
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_SHARED | MmapFlags::MAP_ANONYMOUS,
    )?;
    let private = map_anon(
        0,
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
    )?;

    unsafe { bytes_mut(shared, PAGE_SIZE) }.fill(0);
    unsafe { bytes_mut(private, PAGE_SIZE) }.fill(0);
    store_byte(private, 0, 7);

    let child_pid = fork_like()?;
    if child_pid == 0 {
        store_byte(shared, CHILD_PRIVATE_SNAPSHOT, load_byte(private, 0));
        store_byte(private, 0, 11);

        let grandchild_pid = fork_like()?;
        if grandchild_pid == 0 {
            store_byte(shared, GRANDCHILD_PRIVATE_SNAPSHOT, load_byte(private, 0));
            store_byte(private, 0, 13);
            store_byte(shared, GRANDCHILD_SHARED_WRITE, 77);
            process::exit(0);
        }

        wait_child_exit_ok(grandchild_pid, "multi-fork-nested-lineage/grandchild")?;
        store_byte(
            shared,
            CHILD_PRIVATE_AFTER_GRANDCHILD,
            load_byte(private, 0),
        );
        store_byte(shared, CHILD_SHARED_WRITE, 88);
        process::exit(0);
    }

    store_byte(private, 0, 9);
    wait_child_exit_ok(child_pid, "multi-fork-nested-lineage/child")?;

    assert_eq!(
        load_byte(shared, CHILD_PRIVATE_SNAPSHOT),
        7,
        "child must observe parent's private snapshot"
    );
    assert_eq!(
        load_byte(shared, GRANDCHILD_PRIVATE_SNAPSHOT),
        11,
        "grandchild must observe child's private snapshot"
    );
    assert_eq!(
        load_byte(shared, GRANDCHILD_SHARED_WRITE),
        77,
        "grandchild shared write must reach parent"
    );
    assert_eq!(
        load_byte(shared, CHILD_PRIVATE_AFTER_GRANDCHILD),
        11,
        "child must not observe grandchild private write"
    );
    assert_eq!(
        load_byte(shared, CHILD_SHARED_WRITE),
        88,
        "child shared write must reach parent"
    );
    assert_eq!(
        load_byte(private, 0),
        9,
        "parent must not observe descendant private writes"
    );

    munmap(shared, PAGE_SIZE)?;
    munmap(private, PAGE_SIZE)
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    const LOOP: usize = 4;

    println!("mmap-test: started with pid {}", getpid()?);

    for i in 0..LOOP {
        println!("mmap-test: loop {i}/{LOOP}");
        for (name, test) in TESTS {
            run_test(name, *test)?;
        }
    }

    println!("mmap-test: all cases passed");
    Ok(())
}
