#![no_std]
#![no_main]

use core::{mem::size_of, ptr};

use anemone_rs::{
    abi::{
        fs::linux::{
            STDOUT_FILENO,
            mode::{S_IFDIR, S_IFMT, S_IFREG},
            open::{O_CREAT, O_DIRECTORY, O_RDONLY, O_RDWR},
            seek::{SEEK_END, SEEK_SET},
        },
        syscall::{SYS_GETDENTS64, SYS_LSEEK, SYS_PREAD64, syscall},
        system::native::power::SHUTDOWN_MAGIC,
    },
    alloc::string::ToString,
    os::{
        anemone::power::shutdown,
        linux::{
            fs::{
                AtFd, Fd, close, fstatat, ftruncate, mkdirat, mknodat, mount, mount_with_data,
                openat, read, umount, write,
            },
            tty::{SetTermiosWhen, tcgetattr, tcsetattr},
        },
    },
    prelude::*,
};

const SYS: &str = "/sys";
const SYS2: &str = "/sys2";
const PROC: &str = "/proc";
const ADDRESS: &str = "/sys/kernel/address_bits";
const BYTEORDER: &str = "/sys/kernel/cpu_byteorder";
const ADDRESS2: &str = "/sys2/kernel/address_bits";
const FILESYSTEMS: &str = "/proc/filesystems";

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct DirentHeader {
    ino: u64,
    offset: i64,
    record_len: u16,
    ty: u8,
}

macro_rules! require {
    ($condition:expr, $message:literal) => {
        if !$condition {
            println!("STATIC-SYSFS:FAIL:{}", $message);
            return Err(EINVAL);
        }
    };
}

fn ensure_dir(path: &str) -> Result<(), Errno> {
    match fstatat(AtFd::Cwd, Path::new(path)) {
        Ok(stat) if stat.st_mode & S_IFMT == S_IFDIR => Ok(()),
        Ok(_) => Err(ENOTDIR),
        Err(ENOENT) => mkdirat(AtFd::Cwd, Path::new(path), 0o755),
        Err(error) => Err(error),
    }
}

fn open(path: &str, flags: u32) -> Result<Fd, Errno> {
    openat(AtFd::Cwd, Path::new(path), flags, 0)
}

fn read_all(path: &str) -> Result<Vec<u8>, Errno> {
    let fd = open(path, O_RDONLY)?;
    let mut output = Vec::new();
    let mut buffer = [0u8; 128];
    loop {
        let count = read(fd, &mut buffer)?;
        if count == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..count]);
    }
    close(fd)?;
    Ok(output)
}

fn raw_pread(fd: Fd, buffer: &mut [u8], offset: u64) -> Result<usize, Errno> {
    unsafe {
        syscall(
            SYS_PREAD64,
            fd as u64,
            buffer.as_mut_ptr() as u64,
            buffer.len() as u64,
            offset,
            0,
            0,
        )
    }
    .map(|count| count as usize)
}

fn raw_seek(fd: Fd, offset: i64, whence: usize) -> Result<usize, Errno> {
    unsafe { syscall(SYS_LSEEK, fd as u64, offset as u64, whence as u64, 0, 0, 0) }
        .map(|position| position as usize)
}

fn list(path: &str) -> Result<Vec<String>, Errno> {
    let fd = open(path, O_RDONLY | O_DIRECTORY)?;
    let mut buffer = [0u8; 512];
    let mut names = Vec::new();
    loop {
        let count = unsafe {
            syscall(
                SYS_GETDENTS64,
                fd as u64,
                buffer.as_mut_ptr() as u64,
                buffer.len() as u64,
                0,
                0,
                0,
            )
        }? as usize;
        if count == 0 {
            break;
        }
        let mut offset = 0;
        while offset < count {
            require!(
                count - offset >= size_of::<DirentHeader>(),
                "short-dirent-header"
            );
            let header =
                unsafe { ptr::read_unaligned(buffer.as_ptr().add(offset).cast::<DirentHeader>()) };
            let record_len = header.record_len as usize;
            require!(
                record_len >= size_of::<DirentHeader>() + 1 && offset + record_len <= count,
                "invalid-dirent-length"
            );
            let name_bytes = &buffer[offset + size_of::<DirentHeader>()..offset + record_len];
            let name_len = name_bytes
                .iter()
                .position(|byte| *byte == 0)
                .ok_or(EINVAL)?;
            let name = core::str::from_utf8(&name_bytes[..name_len]).map_err(|_| EINVAL)?;
            if name != "." && name != ".." {
                names.push(name.to_string());
            }
            offset += record_len;
        }
    }
    close(fd)?;
    println!("STATIC-SYSFS:LS:{}:{:?}", path, names);
    Ok(names)
}

fn check_mode(path: &str, kind: u32, permission: u32) -> Result<(), Errno> {
    let stat = fstatat(AtFd::Cwd, Path::new(path))?;
    require!(stat.st_mode & S_IFMT == kind, "wrong-file-kind");
    require!(stat.st_mode & 0o7777 == permission, "wrong-mode");
    Ok(())
}

fn check_read_protocol() -> Result<(), Errno> {
    let fd = open(ADDRESS, O_RDONLY)?;
    let mut buffer = [0u8; 16];
    require!(
        read(fd, &mut buffer[..1])? == 1 && &buffer[..1] == b"6",
        "partial-first"
    );
    require!(
        read(fd, &mut buffer[..2])? == 2 && &buffer[..2] == b"4\n",
        "partial-rest"
    );
    require!(read(fd, &mut buffer)? == 0, "eof");
    require!(
        raw_pread(fd, &mut buffer, 0)? == 3 && &buffer[..3] == b"64\n",
        "pread-zero"
    );
    require!(raw_seek(fd, 0, SEEK_SET)? == 0, "seek-zero");
    require!(
        read(fd, &mut buffer)? == 3 && &buffer[..3] == b"64\n",
        "reread-zero"
    );
    require!(raw_seek(fd, 2, SEEK_END)? == 5, "seek-after-eof");
    require!(read(fd, &mut buffer)? == 0, "read-after-eof");
    close(fd)
}

fn run() -> Result<(), Errno> {
    ensure_dir(SYS)?;
    ensure_dir(SYS2)?;
    ensure_dir(PROC)?;
    mount(Path::new("proc"), Path::new(PROC), "proc")?;

    require!(
        mount_with_data(Path::new("sysfs"), Path::new(SYS), "sysfs", Some("bad")) == Err(EINVAL),
        "nonempty-mount-data"
    );
    mount(Path::new("sysfs"), Path::new(SYS), "sysfs")?;
    mount(Path::new("ignored-source"), Path::new(SYS2), "sysfs")?;

    let root_names = list(SYS)?;
    require!(root_names.as_slice() == ["kernel"], "root-entries");
    let kernel_names = list("/sys/kernel")?;
    require!(
        kernel_names.len() == 2
            && kernel_names.iter().any(|name| name == "address_bits")
            && kernel_names.iter().any(|name| name == "cpu_byteorder"),
        "kernel-entries"
    );
    check_mode(SYS, S_IFDIR, 0o555)?;
    check_mode("/sys/kernel", S_IFDIR, 0o555)?;
    check_mode(ADDRESS, S_IFREG, 0o444)?;
    check_mode(BYTEORDER, S_IFREG, 0o444)?;
    require!(read_all(ADDRESS)? == b"64\n", "address-content");
    require!(read_all(BYTEORDER)? == b"little\n", "byteorder-content");
    require!(read_all(ADDRESS2)? == b"64\n", "second-view-content");
    check_read_protocol()?;

    require!(
        mkdirat(AtFd::Cwd, Path::new("/sys/newdir"), 0o755).is_err(),
        "mkdir-succeeded"
    );
    require!(
        mknodat(AtFd::Cwd, Path::new("/sys/newnode"), S_IFREG | 0o644, 0).is_err(),
        "mknod-succeeded"
    );
    require!(
        openat(
            AtFd::Cwd,
            Path::new("/sys/newfile"),
            O_CREAT | O_RDWR,
            0o644
        )
        .is_err(),
        "create-succeeded"
    );
    let writable = open(ADDRESS, O_RDWR)?;
    require!(write(writable, b"32\n").is_err(), "write-succeeded");
    require!(ftruncate(writable, 0).is_err(), "truncate-succeeded");
    close(writable)?;

    let filesystems = read_all(FILESYSTEMS)?;
    require!(
        filesystems
            .windows(b"nodev\tsysfs\n".len())
            .any(|window| window == b"nodev\tsysfs\n"),
        "registry-projection"
    );

    umount(Path::new(SYS))?;
    require!(read_all(ADDRESS2)? == b"64\n", "surviving-view");
    umount(Path::new(SYS2))?;
    println!("STATIC-SYSFS:REMOUNT:begin");
    mount(Path::new("sysfs"), Path::new(SYS), "sysfs")?;
    println!("STATIC-SYSFS:REMOUNT:mounted");
    let remount_address = read_all(ADDRESS).map_err(|error| {
        println!("STATIC-SYSFS:REMOUNT:address-errno={}", error);
        error
    })?;
    require!(remount_address == b"64\n", "remount-address");
    println!("STATIC-SYSFS:REMOUNT:address");
    require!(read_all(BYTEORDER)? == b"little\n", "remount-byteorder");
    println!("STATIC-SYSFS:REMOUNT:byteorder");
    require!(list(SYS)?.as_slice() == ["kernel"], "remount-namespace");

    println!("STATIC-SYSFS:PASS");
    Ok(())
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    if let Err(error) = run() {
        println!("STATIC-SYSFS:FAIL:errno={}", error);
    }
    // The shutdown syscall can stop the UART before its queued tail reaches
    // the host; drain stdout so the final acceptance result remains observable.
    let termios = tcgetattr(STDOUT_FILENO as Fd)?;
    tcsetattr(STDOUT_FILENO as Fd, SetTermiosWhen::Drain, &termios)?;
    shutdown(SHUTDOWN_MAGIC)?;
    unreachable!("static-sysfs-test: shutdown returned unexpectedly")
}
