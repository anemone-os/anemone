use anemone_rs::{
    abi::fs::linux::open::O_RDONLY,
    os::linux::fs::{getdents64, read},
    prelude::*,
};

use crate::abi::{OwnedFd, open};

const PROC_ROOT: &str = "/proc/nemophila";

fn ensure(condition: bool) -> Result<(), Errno> {
    if condition { Ok(()) } else { Err(EIO) }
}

pub(crate) fn open_instance(identity: u64) -> Result<OwnedFd, Errno> {
    open(&format!("{PROC_ROOT}/{identity}"), O_RDONLY)
}

pub(crate) fn read_all(fd: &OwnedFd) -> Result<String, Errno> {
    let mut bytes = Vec::new();
    let mut batch = [0u8; 256];
    loop {
        let count = read(fd.0, &mut batch)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&batch[..count]);
    }
    String::from_utf8(bytes).map_err(|_| EIO)
}

pub(crate) fn text(identity: u64) -> Result<String, Errno> {
    read_all(&open_instance(identity)?)
}

pub(crate) fn assert_fields(
    identity: u64,
    source: &str,
    artifact: &str,
    lifecycle: &str,
) -> Result<(), Errno> {
    let value = text(identity)?;
    ensure(value.contains(&format!("instance: {identity}\n")))?;
    ensure(value.contains(&format!("source: {source}\n")))?;
    ensure(value.contains(&format!("artifact: {artifact}\n")))?;
    ensure(value.contains(&format!("lifecycle: {lifecycle}\n")))?;
    ensure(value.contains("in_flight: "))
}

pub(crate) fn wait_in_flight(identity: u64) -> Result<(), Errno> {
    for _ in 0..20_000 {
        match text(identity) {
            Ok(value) if !value.contains("in_flight: 0\n") => return Ok(()),
            Ok(_) => {},
            Err(error) => return Err(error),
        }
        anemone_rs::os::linux::process::sched_yield()?;
    }
    Err(ETIMEDOUT)
}

pub(crate) struct DirectorySnapshot(OwnedFd);

impl DirectorySnapshot {
    pub(crate) fn open() -> Result<Self, Errno> {
        open(PROC_ROOT, O_RDONLY).map(Self)
    }

    pub(crate) fn identities(&self) -> Result<Vec<u64>, Errno> {
        let mut identities = Vec::new();
        let mut buffer = [0u8; 1024];
        loop {
            let count = getdents64(self.0.0, &mut buffer)?;
            if count == 0 {
                break;
            }
            let mut offset = 0usize;
            while offset < count {
                if count - offset < 20 {
                    return Err(EIO);
                }
                let record_len =
                    u16::from_ne_bytes([buffer[offset + 16], buffer[offset + 17]]) as usize;
                if record_len < 20 || record_len > count - offset {
                    return Err(EIO);
                }
                let name_bytes = &buffer[offset + 19..offset + record_len];
                let name_len = name_bytes.iter().position(|byte| *byte == 0).ok_or(EIO)?;
                let name = core::str::from_utf8(&name_bytes[..name_len]).map_err(|_| EIO)?;
                if name != "." && name != ".." {
                    identities.push(name.parse::<u64>().map_err(|_| EIO)?);
                }
                offset += record_len;
            }
        }
        Ok(identities)
    }
}
