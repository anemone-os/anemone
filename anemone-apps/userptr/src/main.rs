#![no_main]
#![no_std]

use core::{mem::size_of, ptr};

use anemone_rs::{
    abi::{
        process::linux::{signal as linux_signal, wait},
        syscall::*,
    },
    os::linux::{
        fs::{self, Fd, PipeFlags},
        process::{self, MmapFlags, MmapProt, mmap, mprotect, munmap},
    },
    prelude::*,
};

const PAGE_SIZE: usize = 4096;
const BAD_LOW: u64 = 1;
const BAD_HIGH: u64 = u64::MAX;
const AT_FDCWD: u64 = (-100i64) as u64;

type TestFn = fn() -> Result<(), Errno>;

const TESTS: &[(&str, TestFn)] = &[
    ("scalar-bad-addresses", test_scalar_bad_addresses),
    ("lazy-and-cross-page", test_lazy_and_cross_page),
    ("partial-cross-page-fault", test_partial_cross_page_fault),
    ("permissions-and-unmap", test_permissions_and_unmap),
    ("side-effects-after-efault", test_side_effects_after_efault),
    ("wait4-copyout-order", test_wait4_copyout_order),
    ("getcpu-copyout", test_getcpu_copyout),
];

unsafe fn syscall6(sysno: u64, args: [u64; 6]) -> Result<u64, Errno> {
    unsafe { syscall(sysno, args[0], args[1], args[2], args[3], args[4], args[5]) }
}

#[track_caller]
fn expect_errno(name: &str, result: Result<u64, Errno>, expected: Errno) {
    match result {
        Err(actual) => assert_eq!(actual, expected, "{name}: unexpected errno"),
        Ok(value) => panic!("{name}: expected {expected}, got success {value}"),
    }
}

#[track_caller]
fn expect_count(name: &str, result: Result<u64, Errno>, expected: u64) {
    match result {
        Ok(actual) => assert_eq!(actual, expected, "{name}: unexpected byte count"),
        Err(error) => panic!("{name}: expected {expected} bytes, got {error}"),
    }
}

fn map_pages(count: usize, prot: MmapProt) -> Result<*mut u8, Errno> {
    mmap(
        0,
        PAGE_SIZE * count,
        prot,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )
    .map(|mapping| mapping.as_ptr())
}

fn close_pair(pair: (Fd, Fd)) {
    let _ = fs::close(pair.0);
    let _ = fs::close(pair.1);
}

fn test_scalar_bad_addresses() -> Result<(), Errno> {
    let pipe = fs::pipe2(PipeFlags::empty())?;
    fs::write(pipe.1, b"r")?;

    unsafe {
        expect_errno(
            "write reads BAD_LOW",
            syscall6(SYS_WRITE, [pipe.1 as u64, BAD_LOW, 1, 0, 0, 0]),
            EFAULT,
        );
        expect_errno(
            "write rejects BAD_HIGH",
            syscall6(SYS_WRITE, [pipe.1 as u64, BAD_HIGH, 1, 0, 0, 0]),
            EFAULT,
        );
        expect_errno(
            "read writes BAD_LOW",
            syscall6(SYS_READ, [pipe.0 as u64, BAD_LOW, 1, 0, 0, 0]),
            EFAULT,
        );
        expect_errno(
            "pipe2 writes BAD_LOW",
            syscall6(SYS_PIPE2, [BAD_LOW, 0, 0, 0, 0, 0]),
            EFAULT,
        );
        expect_errno(
            "fstat writes BAD_LOW",
            syscall6(SYS_FSTAT, [pipe.0 as u64, BAD_LOW, 0, 0, 0, 0]),
            EFAULT,
        );
        expect_errno(
            "getcwd writes BAD_LOW",
            syscall6(SYS_GETCWD, [BAD_LOW, 64, 0, 0, 0, 0]),
            EFAULT,
        );
        expect_errno(
            "gettimeofday writes BAD_LOW",
            syscall6(SYS_GETTIMEOFDAY, [BAD_LOW, 0, 0, 0, 0, 0]),
            EFAULT,
        );
        expect_errno(
            "clock_gettime writes BAD_LOW",
            syscall6(SYS_CLOCK_GETTIME, [1, BAD_LOW, 0, 0, 0, 0]),
            EFAULT,
        );
        expect_errno(
            "uname writes BAD_LOW",
            syscall6(SYS_UNAME, [BAD_LOW, 0, 0, 0, 0, 0]),
            EFAULT,
        );
        expect_errno(
            "getrandom writes BAD_LOW",
            syscall6(SYS_GETRANDOM, [BAD_LOW, 16, 0, 0, 0, 0]),
            EFAULT,
        );
        expect_errno(
            "openat reads BAD_LOW path",
            syscall6(SYS_OPENAT, [(-100i64) as u64, BAD_LOW, 0, 0, 0, 0]),
            EFAULT,
        );
        expect_errno(
            "nanosleep reads BAD_LOW",
            syscall6(SYS_NANOSLEEP, [BAD_LOW, 0, 0, 0, 0, 0]),
            EFAULT,
        );
        expect_errno(
            "rt_sigaction reads BAD_LOW",
            syscall6(
                SYS_RT_SIGACTION,
                [
                    linux_signal::SIGUSR1 as u64,
                    BAD_LOW,
                    0,
                    size_of::<linux_signal::SigSet>() as u64,
                    0,
                    0,
                ],
            ),
            EFAULT,
        );
        expect_errno(
            "rt_sigaction writes BAD_LOW",
            syscall6(
                SYS_RT_SIGACTION,
                [
                    linux_signal::SIGUSR1 as u64,
                    0,
                    BAD_LOW,
                    size_of::<linux_signal::SigSet>() as u64,
                    0,
                    0,
                ],
            ),
            EFAULT,
        );
    }

    close_pair(pipe);
    Ok(())
}

fn test_lazy_and_cross_page() -> Result<(), Errno> {
    let pipefd_page = map_pages(1, MmapProt::PROT_READ | MmapProt::PROT_WRITE)?;
    expect_count(
        "pipe2 lazy copyout",
        unsafe { syscall6(SYS_PIPE2, [pipefd_page as u64, 0, 0, 0, 0, 0]) },
        0,
    );
    let lazy_pipe = unsafe {
        (
            ptr::read_volatile(pipefd_page.cast::<i32>()) as Fd,
            ptr::read_volatile(pipefd_page.add(size_of::<i32>()).cast::<i32>()) as Fd,
        )
    };
    close_pair(lazy_pipe);
    munmap(pipefd_page, PAGE_SIZE)?;

    let dst = map_pages(2, MmapProt::PROT_READ | MmapProt::PROT_WRITE)?;
    let cross = unsafe { dst.add(PAGE_SIZE - 4) };
    let pipe = fs::pipe2(PipeFlags::empty())?;
    assert_eq!(fs::write(pipe.1, b"abcdefgh")?, 8);
    expect_count(
        "read lazy cross-page copyout",
        unsafe { syscall6(SYS_READ, [pipe.0 as u64, cross as u64, 8, 0, 0, 0]) },
        8,
    );
    for (index, expected) in b"abcdefgh".iter().copied().enumerate() {
        assert_eq!(unsafe { ptr::read_volatile(cross.add(index)) }, expected);
    }

    expect_count(
        "write cross-page copyin",
        unsafe { syscall6(SYS_WRITE, [pipe.1 as u64, cross as u64, 8, 0, 0, 0]) },
        8,
    );

    close_pair(pipe);
    munmap(dst, PAGE_SIZE * 2)
}

fn test_partial_cross_page_fault() -> Result<(), Errno> {
    let source = map_pages(2, MmapProt::PROT_READ | MmapProt::PROT_WRITE)?;
    let source_cross = unsafe { source.add(PAGE_SIZE - 4) };
    for (index, byte) in b"12345678".iter().copied().enumerate() {
        unsafe { ptr::write_volatile(source_cross.add(index), byte) };
    }
    mprotect(
        unsafe { source.add(PAGE_SIZE) },
        PAGE_SIZE,
        MmapProt::empty(),
    )?;

    let source_pipe = fs::pipe2(PipeFlags::empty())?;
    expect_count(
        "write stops at protected second page",
        unsafe {
            syscall6(
                SYS_WRITE,
                [source_pipe.1 as u64, source_cross as u64, 8, 0, 0, 0],
            )
        },
        4,
    );
    let mut written_prefix = [0u8; 4];
    expect_count(
        "partial write commits only copied prefix",
        unsafe {
            syscall6(
                SYS_READ,
                [
                    source_pipe.0 as u64,
                    written_prefix.as_mut_ptr() as u64,
                    written_prefix.len() as u64,
                    0,
                    0,
                    0,
                ],
            )
        },
        written_prefix.len() as u64,
    );
    assert_eq!(&written_prefix, b"1234");

    expect_errno(
        "exact rt_sigaction copyin crosses protected page",
        unsafe {
            syscall6(
                SYS_RT_SIGACTION,
                [
                    linux_signal::SIGUSR1 as u64,
                    source_cross as u64,
                    0,
                    size_of::<linux_signal::SigSet>() as u64,
                    0,
                    0,
                ],
            )
        },
        EFAULT,
    );
    close_pair(source_pipe);
    munmap(source, PAGE_SIZE * 2)?;

    let destination = map_pages(2, MmapProt::PROT_READ | MmapProt::PROT_WRITE)?;
    mprotect(
        unsafe { destination.add(PAGE_SIZE) },
        PAGE_SIZE,
        MmapProt::empty(),
    )?;
    let destination_cross = unsafe { destination.add(PAGE_SIZE - 4) };
    let destination_pipe = fs::pipe2(PipeFlags::empty())?;
    assert_eq!(fs::write(destination_pipe.1, b"ABCDEFGH")?, 8);
    expect_count(
        "read stops at protected second page",
        unsafe {
            syscall6(
                SYS_READ,
                [
                    destination_pipe.0 as u64,
                    destination_cross as u64,
                    8,
                    0,
                    0,
                    0,
                ],
            )
        },
        4,
    );
    let mut unread_suffix = [0u8; 4];
    expect_count(
        "partial read leaves uncommitted pipe suffix",
        unsafe {
            syscall6(
                SYS_READ,
                [
                    destination_pipe.0 as u64,
                    unread_suffix.as_mut_ptr() as u64,
                    unread_suffix.len() as u64,
                    0,
                    0,
                    0,
                ],
            )
        },
        unread_suffix.len() as u64,
    );
    assert_eq!(&unread_suffix, b"EFGH");
    close_pair(destination_pipe);
    munmap(destination, PAGE_SIZE * 2)
}

fn test_permissions_and_unmap() -> Result<(), Errno> {
    let input = map_pages(1, MmapProt::empty())?;
    let pipe = fs::pipe2(PipeFlags::empty())?;
    expect_errno(
        "write reads PROT_NONE",
        unsafe { syscall6(SYS_WRITE, [pipe.1 as u64, input as u64, 1, 0, 0, 0]) },
        EFAULT,
    );
    munmap(input, PAGE_SIZE)?;

    let output = map_pages(1, MmapProt::PROT_READ | MmapProt::PROT_WRITE)?;
    unsafe { ptr::write_volatile(output, 0) };
    mprotect(output, PAGE_SIZE, MmapProt::PROT_READ)?;
    expect_errno(
        "gettimeofday writes read-only page",
        unsafe { syscall6(SYS_GETTIMEOFDAY, [output as u64, 0, 0, 0, 0, 0]) },
        EFAULT,
    );
    munmap(output, PAGE_SIZE)?;

    let stale = map_pages(1, MmapProt::PROT_READ | MmapProt::PROT_WRITE)?;
    munmap(stale, PAGE_SIZE)?;
    expect_errno(
        "fstat writes unmapped address",
        unsafe { syscall6(SYS_FSTAT, [pipe.0 as u64, stale as u64, 0, 0, 0, 0]) },
        EFAULT,
    );

    close_pair(pipe);
    Ok(())
}

fn test_side_effects_after_efault() -> Result<(), Errno> {
    let pipe = fs::pipe2(PipeFlags::empty())?;
    fs::write(pipe.1, b"x")?;
    expect_errno(
        "bad read does not consume pipe data",
        unsafe { syscall6(SYS_READ, [pipe.0 as u64, BAD_LOW, 1, 0, 0, 0]) },
        EFAULT,
    );
    let mut byte = 0u8;
    expect_count(
        "pipe data remains after EFAULT",
        unsafe {
            syscall6(
                SYS_READ,
                [pipe.0 as u64, (&mut byte as *mut u8) as u64, 1, 0, 0, 0],
            )
        },
        1,
    );
    assert_eq!(byte, b'x');
    close_pair(pipe);

    let root = b"/\0";
    let dirfd =
        unsafe { syscall6(SYS_OPENAT, [AT_FDCWD, root.as_ptr() as u64, 0, 0, 0, 0])? } as Fd;
    expect_errno(
        "bad getdents64 does not advance directory",
        unsafe { syscall6(SYS_GETDENTS64, [dirfd as u64, BAD_LOW, 512, 0, 0, 0]) },
        EFAULT,
    );

    let dirents = map_pages(1, MmapProt::PROT_READ | MmapProt::PROT_WRITE)?;
    let written =
        unsafe { syscall6(SYS_GETDENTS64, [dirfd as u64, dirents as u64, 512, 0, 0, 0])? };
    assert!(written > 0, "directory cursor advanced across EFAULT");
    munmap(dirents, PAGE_SIZE)?;
    fs::close(dirfd)
}

fn test_wait4_copyout_order() -> Result<(), Errno> {
    let gate = fs::pipe2(PipeFlags::empty())?;
    let child = match process::fork()? {
        Some(pid) => pid,
        None => {
            fs::close(gate.1).expect("child failed to close gate writer");
            let mut release = [0u8; 1];
            fs::read(gate.0, &mut release).expect("child failed to wait on gate");
            process::exit(23)
        },
    };

    fs::close(gate.0)?;
    expect_count(
        "wait4 WNOHANG does not touch status without a result",
        unsafe {
            syscall6(
                SYS_WAIT4,
                [child as u64, BAD_LOW, wait::WNOHANG as u64, 0, 0, 0],
            )
        },
        0,
    );
    fs::write(gate.1, b"x")?;
    fs::close(gate.1)?;

    expect_errno(
        "wait4 reports status copyout failure",
        unsafe { syscall6(SYS_WAIT4, [child as u64, BAD_LOW, 0, 0, 0, 0]) },
        EFAULT,
    );
    expect_errno(
        "wait4 copyout failure still consumes child",
        unsafe { syscall6(SYS_WAIT4, [child as u64, 0, 0, 0, 0, 0]) },
        ECHILD,
    );
    Ok(())
}

fn test_getcpu_copyout() -> Result<(), Errno> {
    let mut cpu = u32::MAX;
    let mut node = u32::MAX;
    expect_count(
        "getcpu writes CPU and node while ignoring tcache",
        unsafe {
            syscall6(
                SYS_GETCPU,
                [
                    (&mut cpu as *mut u32) as u64,
                    (&mut node as *mut u32) as u64,
                    BAD_HIGH,
                    0,
                    0,
                    0,
                ],
            )
        },
        0,
    );
    assert_ne!(cpu, u32::MAX, "getcpu did not write the CPU output");
    assert_eq!(node, 0, "getcpu returned a NUMA node on a non-NUMA system");

    expect_count(
        "getcpu accepts both output pointers as null",
        unsafe { syscall6(SYS_GETCPU, [0, 0, BAD_HIGH, 0, 0, 0]) },
        0,
    );

    node = u32::MAX;
    expect_errno(
        "getcpu reports a bad CPU pointer",
        unsafe {
            syscall6(
                SYS_GETCPU,
                [BAD_LOW, (&mut node as *mut u32) as u64, 0, 0, 0, 0],
            )
        },
        EFAULT,
    );
    assert_eq!(node, 0, "bad CPU pointer suppressed the node copyout");

    cpu = u32::MAX;
    expect_errno(
        "getcpu reports a bad node pointer",
        unsafe {
            syscall6(
                SYS_GETCPU,
                [(&mut cpu as *mut u32) as u64, BAD_LOW, 0, 0, 0, 0],
            )
        },
        EFAULT,
    );
    assert_ne!(cpu, u32::MAX, "bad node pointer suppressed the CPU copyout");
    Ok(())
}

fn run_test(name: &str, test: TestFn) -> Result<(), Errno> {
    println!("userptr: CASE {name} start");
    test()?;
    println!("userptr: CASE {name} ok");
    Ok(())
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    for (name, test) in TESTS {
        run_test(name, *test)?;
    }
    println!("userptr: all cases passed");
    Ok(())
}
