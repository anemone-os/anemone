#![no_std]
#![no_main]

use anemone_rs::{
    abi::fs::linux::open::O_RDONLY,
    env::args,
    io::Read,
    os::{
        anemone::nemophila::{load_embedded, load_supplied, try_unload},
        linux::fs::{AtFd, Fd, close, getdents64, openat},
    },
    prelude::*,
};
const PROC_ROOT: &str = "/proc/nemophila";

struct OwnedFd(Fd);

impl Drop for OwnedFd {
    fn drop(&mut self) {
        let _ = close(self.0);
    }
}

fn open_readonly(path: &str) -> Result<OwnedFd, Errno> {
    openat(AtFd::Cwd, Path::new(path), O_RDONLY, 0).map(OwnedFd)
}

fn validate_identity(identity: u64) -> Result<u64, Errno> {
    if identity == 0 {
        return Err(EINVAL);
    }
    Ok(identity)
}

enum Command<'a> {
    LoadEmbedded(&'a str),
    Load(&'a str),
    TryUnload(u64),
    List,
    Show(u64),
}

fn parse_command<'a>(mut args: impl Iterator<Item = &'a str>) -> Result<Command<'a>, Errno> {
    let command = args.next().ok_or(EINVAL)?;
    let command = match command {
        "load-embedded" => Ok(Command::LoadEmbedded(args.next().ok_or(EINVAL)?)),
        "load" => Ok(Command::Load(args.next().ok_or(EINVAL)?)),
        "try-unload" => Ok(Command::TryUnload(
            args.next().ok_or(EINVAL)?.parse().map_err(|_| EINVAL)?,
        )),
        "list" => Ok(Command::List),
        "show" => Ok(Command::Show(
            args.next().ok_or(EINVAL)?.parse().map_err(|_| EINVAL)?,
        )),
        _ => Err(EINVAL),
    }?;
    if args.next().is_some() {
        return Err(EINVAL);
    }
    Ok(command)
}

fn print_file(path: &str) -> Result<(), Errno> {
    let mut file = anemone_rs::fs::OpenOptions::new()
        .read(true)
        .open(Path::new(path))?;
    let mut bytes = Vec::new();
    let mut batch = [0u8; 512];
    loop {
        let read = file.read(&mut batch)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&batch[..read]);
    }
    let text = core::str::from_utf8(&bytes).map_err(|_| EIO)?;
    print!("{text}");
    Ok(())
}

fn list() -> Result<(), Errno> {
    let directory = open_readonly(PROC_ROOT)?;
    let mut buffer = [0u8; 1024];
    loop {
        let count = getdents64(directory.0, &mut buffer)?;
        if count == 0 {
            break;
        }
        let mut offset = 0usize;
        while offset < count {
            if count - offset < 19 {
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
                println!("{name}");
            }
            offset += record_len;
        }
    }
    Ok(())
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    match parse_command(args().skip(1))? {
        Command::LoadEmbedded(artifact) => {
            println!("{}", load_embedded(artifact)?);
        },
        Command::Load(path) => {
            let file = open_readonly(path)?;
            println!("{}", load_supplied(file.0 as i32)?);
        },
        Command::TryUnload(instance) => {
            try_unload(validate_identity(instance)?)?;
        },
        Command::List => {
            list()?;
        },
        Command::Show(instance) => {
            let identity = validate_identity(instance)?;
            print_file(&format!("{PROC_ROOT}/{identity}"))?;
        },
    }
    Ok(())
}
