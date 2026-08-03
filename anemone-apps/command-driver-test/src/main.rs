#![no_std]
#![no_main]

use anemone_rs::{
    abi::system::native::power::SHUTDOWN_MAGIC,
    os::{
        anemone::power::shutdown,
        linux::{
            fs::STDOUT_FILENO,
            process::{WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork, wait4},
            tty::{SetTermiosWhen, tcgetattr, tcsetattr},
        },
    },
    prelude::*,
};

fn ensure(condition: bool) -> Result<(), Errno> {
    if condition { Ok(()) } else { Err(EIO) }
}

fn run_app(path: &str, name: &str) -> Result<(), Errno> {
    match fork()? {
        None => {
            let errno = execve(path, &[name], &[]).unwrap_err();
            exit(errno as i8)
        },
        Some(child) => {
            let mut status = WStatusRaw::EMPTY;
            ensure(
                wait4(
                    WaitFor::ChildWithTgid(child),
                    Some(&mut status),
                    WaitOptions::empty(),
                )? == Some(child)
                    && matches!(status.read(), WStatus::Exited(0)),
            )
        },
    }
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    println!("command-driver-test: CASE c-and-cpp start");
    run_app("/bin/command-c", "command-c")?;
    run_app("/bin/command-cpp", "command-cpp")?;
    println!("command-driver-test: CASE c-and-cpp ok");

    let termios = tcgetattr(STDOUT_FILENO as _)?;
    tcsetattr(STDOUT_FILENO as _, SetTermiosWhen::Drain, &termios)?;
    shutdown(SHUTDOWN_MAGIC)?;
    unreachable!("command-driver-test: shutdown returned unexpectedly");
}
