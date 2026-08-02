#![no_std]
#![no_main]

use anemone_rs::{
    abi::{
        syscall::{linux::SYS_SETUID, syscall},
        system::native::power::SHUTDOWN_MAGIC,
    },
    os::{
        anemone::{
            debug::{LogLevels, get_log_levels, set_log_levels},
            power::shutdown,
        },
        linux::{
            fs::STDOUT_FILENO,
            process::{WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork, wait4},
            tty::{SetTermiosWhen, tcgetattr, tcsetattr},
        },
    },
    prelude::*,
    process::process_id,
};

const DEBUG_LEVELS: LogLevels = LogLevels::new(7, 7);

fn setuid(uid: u32) -> Result<(), Errno> {
    unsafe { syscall(SYS_SETUID, uid as u64, 0, 0, 0, 0, 0) }.map(|_| ())
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

fn unprivileged_child() -> ! {
    let passed = setuid(1000).is_ok()
        && get_log_levels() == Ok(DEBUG_LEVELS)
        && set_log_levels(LogLevels::new(8, 0)) == Err(EINVAL)
        && get_log_levels() == Ok(DEBUG_LEVELS)
        && set_log_levels(DEBUG_LEVELS) == Err(EPERM)
        && get_log_levels() == Ok(DEBUG_LEVELS);
    exit(if passed { 0 } else { 1 })
}

fn exercise_policy() -> Result<(), Errno> {
    ensure(get_log_levels()? == DEBUG_LEVELS)?;

    ensure(set_log_levels(LogLevels::new(8, 0)) == Err(EINVAL))?;
    ensure(get_log_levels()? == DEBUG_LEVELS)?;
    ensure(set_log_levels(LogLevels::new(6, 7)) == Err(EINVAL))?;
    ensure(get_log_levels()? == DEBUG_LEVELS)?;

    match fork()? {
        Some(child) => wait_child(child),
        None => unprivileged_child(),
    }
}

fn run() -> Result<(), Errno> {
    let old = get_log_levels()?;
    let replaced = set_log_levels(DEBUG_LEVELS)?;
    if replaced != old {
        let _ = set_log_levels(old);
        return Err(EIO);
    }

    // Every fallible exercise result is retained until after the explicit
    // restore. The kernel deliberately does not infer ownership from this
    // process or restore policy during exit.
    let exercise = exercise_policy();
    let restore = set_log_levels(old);
    let restored_from = restore?;
    ensure(restored_from == DEBUG_LEVELS)?;
    exercise?;
    ensure(get_log_levels()? == old)?;
    Ok(())
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    println!("log-test: CASE get-set-invalid-permission-restore start");
    run()?;
    println!("log-test: CASE get-set-invalid-permission-restore ok");

    if process_id() == 1 {
        // TTY writes complete when queued. Drain the final acceptance line
        // before shutdown so a fast platform cannot discard its evidence.
        let termios = tcgetattr(STDOUT_FILENO as _)?;
        tcsetattr(STDOUT_FILENO as _, SetTermiosWhen::Drain, &termios)?;
        shutdown(SHUTDOWN_MAGIC)?;
        unreachable!("log-test: shutdown returned unexpectedly");
    }
    Ok(())
}
