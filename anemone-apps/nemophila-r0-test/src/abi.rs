use anemone_rs::{
    abi::{
        RawUserAddr64,
        fs::linux::open::{O_CREAT, O_DIRECTORY, O_PATH, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY},
        nemophila::{
            LOAD_FLAGS_NONE, LOAD_REQUEST_SIZE, LOAD_SOURCE_EMBEDDED, LOAD_SOURCE_SUPPLIED_FD,
            LoadRequest,
        },
        syscall::{SYS_NEMOPHILA_LOAD, SYS_NEMOPHILA_TRY_UNLOAD, syscall},
    },
    os::{
        anemone::nemophila::{load_embedded, load_supplied},
        linux::{
            fs::{AtFd, Fd, PipeFlags, close, ftruncate, openat, pipe2, write},
            process::{WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork, setuid, wait4},
        },
    },
    prelude::*,
};

pub(crate) const ARTIFACT: &str = "/modules/clone-observer.wasm";
pub(crate) const MUTABLE_ARTIFACT: &str = "/modules/clone-observer-mutable.wasm";

pub(crate) struct OwnedFd(pub(crate) Fd);

impl Drop for OwnedFd {
    fn drop(&mut self) {
        let _ = close(self.0);
    }
}

pub(crate) fn open(path: &str, flags: u32) -> Result<OwnedFd, Errno> {
    openat(AtFd::Cwd, Path::new(path), flags, 0).map(OwnedFd)
}

pub(crate) fn request(source_kind: u32) -> LoadRequest {
    LoadRequest {
        size: LOAD_REQUEST_SIZE,
        source_kind,
        flags: LOAD_FLAGS_NONE,
        payload: RawUserAddr64::NULL,
        payload_len: 0,
        reserved: [0; 2],
    }
}

pub(crate) fn raw_load(request: &LoadRequest) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_NEMOPHILA_LOAD,
            request as *const LoadRequest as u64,
            0,
            0,
            0,
            0,
            0,
        )
    }
}

pub(crate) fn raw_try_unload(identity: u64, flags: u64) -> Result<(), Errno> {
    unsafe { syscall(SYS_NEMOPHILA_TRY_UNLOAD, identity, flags, 0, 0, 0, 0).map(|_| ()) }
}

fn ensure(condition: bool) -> Result<(), Errno> {
    if condition { Ok(()) } else { Err(EIO) }
}

fn wait_child(child: u32) -> Result<(), Errno> {
    let mut status = WStatusRaw::EMPTY;
    ensure(
        wait4(
            WaitFor::ChildWithTgid(child),
            Some(&mut status),
            WaitOptions::empty(),
        )? == Some(child),
    )?;
    ensure(matches!(status.read(), WStatus::Exited(0)))
}

fn unprivileged_authorization() -> ! {
    let mut supplied = request(LOAD_SOURCE_SUPPLIED_FD);
    supplied.payload = RawUserAddr64::from_bits(u64::MAX);
    let passed = setuid(1000).is_ok()
        && unsafe { syscall(SYS_NEMOPHILA_LOAD, 1, 0, 0, 0, 0, 0) } == Err(EPERM)
        && raw_load(&supplied) == Err(EPERM)
        && raw_try_unload(u64::MAX, 1) == Err(EPERM);
    exit(if passed { 0 } else { 1 })
}

fn authorization_precedes_copy_and_control_validation() -> Result<(), Errno> {
    match fork()? {
        Some(child) => wait_child(child),
        None => unprivileged_authorization(),
    }
}

fn request_validation() -> Result<(), Errno> {
    ensure(unsafe { syscall(SYS_NEMOPHILA_LOAD, 1, 0, 0, 0, 0, 0) } == Err(EFAULT))?;

    let mut value = request(LOAD_SOURCE_EMBEDDED);
    value.size -= 1;
    ensure(raw_load(&value) == Err(EINVAL))?;
    value = request(u32::MAX);
    ensure(raw_load(&value) == Err(EINVAL))?;
    value = request(LOAD_SOURCE_EMBEDDED);
    value.flags = 1;
    ensure(raw_load(&value) == Err(EINVAL))?;
    value = request(LOAD_SOURCE_EMBEDDED);
    value.reserved[0] = 1;
    ensure(raw_load(&value) == Err(EINVAL))?;
    value = request(LOAD_SOURCE_EMBEDDED);
    value.payload = RawUserAddr64::from_bits(1);
    value.payload_len = 1;
    ensure(raw_load(&value) == Err(EFAULT))?;
    value = request(LOAD_SOURCE_SUPPLIED_FD);
    value.payload_len = 1;
    ensure(raw_load(&value) == Err(EINVAL))?;

    ensure(load_embedded("-invalid") == Err(EINVAL))?;
    ensure(load_embedded("missing-artifact") == Err(ENOENT))?;
    ensure(raw_try_unload(0, 0) == Err(EINVAL))?;
    ensure(raw_try_unload(u64::MAX, 1) == Err(EINVAL))?;
    Ok(())
}

fn create(path: &str) -> Result<OwnedFd, Errno> {
    open(path, O_CREAT | O_TRUNC | O_RDWR)
}

fn source_rejection() -> Result<(), Errno> {
    ensure(load_supplied(-1) == Err(EBADF))?;

    let write_only = open(ARTIFACT, O_WRONLY)?;
    ensure(load_supplied(write_only.0 as i32) == Err(EBADF))?;
    let path_only = open(ARTIFACT, O_PATH)?;
    ensure(load_supplied(path_only.0 as i32) == Err(EBADF))?;
    let directory = open("/modules", O_RDONLY | O_DIRECTORY)?;
    ensure(load_supplied(directory.0 as i32) == Err(EINVAL))?;
    let (pipe_read, pipe_write) = pipe2(PipeFlags::empty())?;
    let pipe_read = OwnedFd(pipe_read);
    let _pipe_write = OwnedFd(pipe_write);
    ensure(load_supplied(pipe_read.0 as i32) == Err(EINVAL))?;
    let character = open("/dev/console", O_RDONLY)?;
    ensure(load_supplied(character.0 as i32) == Err(EINVAL))?;

    let empty = create("/tmp/nemophila-empty.wasm")?;
    ensure(load_supplied(empty.0 as i32) == Err(ENOEXEC))?;
    let malformed = create("/tmp/nemophila-malformed.wasm")?;
    ensure(write(malformed.0, b"not wasm")? == 8)?;
    ensure(load_supplied(malformed.0 as i32) == Err(ENOEXEC))?;
    let oversized = create("/tmp/nemophila-oversized.wasm")?;
    ftruncate(oversized.0, 1_048_577)?;
    ensure(load_supplied(oversized.0 as i32) == Err(EFBIG))?;
    Ok(())
}

pub(crate) fn run() -> Result<(), Errno> {
    println!("NEMOPHILA-R0:CASE:ABI-AUTH:START");
    authorization_precedes_copy_and_control_validation()?;
    request_validation()?;
    source_rejection()?;
    println!("NEMOPHILA-R0:CASE:ABI-AUTH:PASS");
    Ok(())
}
