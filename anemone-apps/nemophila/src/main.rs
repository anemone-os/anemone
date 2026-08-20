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

enum Command<'a> {
    Help,
    LoadEmbedded(&'a str),
    Load(&'a str),
    Unload(u64),
    List,
    Show(u64),
}

impl Command<'_> {
    fn name(&self) -> &'static str {
        match self {
            Self::Help => "help",
            Self::LoadEmbedded(_) => "load-embedded",
            Self::Load(_) => "load",
            Self::Unload(_) => "unload",
            Self::List => "list",
            Self::Show(_) => "show",
        }
    }
}

enum CliError<'a> {
    UnknownCommand(&'a str),
    MissingArgument(&'static str),
    UnexpectedArgument(&'a str),
    InvalidIdentity(&'a str),
    ZeroIdentity,
}

fn parse_identity(value: &str) -> Result<u64, CliError<'_>> {
    let identity = value
        .parse()
        .map_err(|_| CliError::InvalidIdentity(value))?;
    if identity == 0 {
        return Err(CliError::ZeroIdentity);
    }
    Ok(identity)
}

fn require_argument<'a>(
    args: &mut impl Iterator<Item = &'a str>,
    name: &'static str,
) -> Result<&'a str, CliError<'a>> {
    args.next().ok_or(CliError::MissingArgument(name))
}

fn parse_command<'a>(mut args: impl Iterator<Item = &'a str>) -> Result<Command<'a>, CliError<'a>> {
    let Some(command) = args.next() else {
        return Ok(Command::Help);
    };
    let command = match command {
        "help" | "-h" | "--help" => Command::Help,
        "load-embedded" => Command::LoadEmbedded(require_argument(&mut args, "ARTIFACT")?),
        "load" => Command::Load(require_argument(&mut args, "PATH")?),
        "unload" => Command::Unload(parse_identity(require_argument(&mut args, "INSTANCE_ID")?)?),
        "list" => Command::List,
        "show" => Command::Show(parse_identity(require_argument(&mut args, "INSTANCE_ID")?)?),
        _ => return Err(CliError::UnknownCommand(command)),
    };
    if let Some(argument) = args.next() {
        return Err(CliError::UnexpectedArgument(argument));
    }
    Ok(command)
}

fn print_help() {
    println!(
        "Nemophila module management\n\
         \n\
         Usage: nemophila <COMMAND> [ARGUMENT]\n\
         \n\
         Commands:\n\
           load-embedded <ARTIFACT>  Load an embedded module by artifact identity\n\
           load <PATH>               Load a module from a file\n\
           unload <INSTANCE_ID>      Unload an idle live or poisoned instance\n\
           list                      List published instance IDs\n\
           show <INSTANCE_ID>        Show one instance snapshot\n\
           help                      Print this help\n\
         \n\
         Options:\n\
           -h, --help                Print this help"
    );
}

fn report_cli_error(error: CliError<'_>) {
    match error {
        CliError::UnknownCommand(command) => {
            eprintln!("nemophila: unknown command '{command}'")
        },
        CliError::MissingArgument(argument) => {
            eprintln!("nemophila: missing required argument <{argument}>")
        },
        CliError::UnexpectedArgument(argument) => {
            eprintln!("nemophila: unexpected argument '{argument}'")
        },
        CliError::InvalidIdentity(identity) => {
            eprintln!("nemophila: invalid instance ID '{identity}'")
        },
        CliError::ZeroIdentity => eprintln!("nemophila: instance ID must be non-zero"),
    }
    eprintln!("Try 'nemophila --help' for more information.");
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
    let command = match parse_command(args().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            report_cli_error(error);
            return Err(EINVAL);
        },
    };
    let command_name = command.name();
    let result = match command {
        Command::Help => {
            print_help();
            Ok(())
        },
        Command::LoadEmbedded(artifact) => {
            load_embedded(artifact).map(|instance| println!("{instance}"))
        },
        Command::Load(path) => open_readonly(path)
            .and_then(|file| load_supplied(file.0 as i32))
            .map(|instance| println!("{instance}")),
        Command::Unload(instance) => try_unload(instance),
        Command::List => list(),
        Command::Show(instance) => print_file(&format!("{PROC_ROOT}/{instance}")),
    };
    if let Err(errno) = result {
        eprintln!("nemophila: {command_name} failed: errno {errno}");
    }
    result
}
