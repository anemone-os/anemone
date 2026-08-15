#![no_std]
#![no_main]

mod abi;
mod lifecycle;
mod procfs;

use anemone_rs::{
    abi::system::native::power::SHUTDOWN_MAGIC,
    os::{
        anemone::{
            debug::{LogLevels, get_log_levels, set_log_levels},
            power::shutdown,
        },
        linux::{
            fs::mount,
            tty::{SetTermiosWhen, tcgetattr, tcsetattr},
        },
    },
    prelude::*,
};

const STDOUT_FILENO: u32 = 1;

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    println!("NEMOPHILA-R0:VALIDATION:START");
    mount(Path::new("devfs"), Path::new("/dev"), "devfs")?;
    mount(Path::new("proc"), Path::new("/proc"), "proc")?;
    abi::run()?;
    lifecycle::run()?;

    // The validation kernel prints Debug records from both CPUs. Silence only
    // console projection while publishing the host oracle so kernel printk
    // cannot splice bytes into the userspace marker on the shared UART.
    let levels = get_log_levels()?;
    let previous = set_log_levels(LogLevels::new(levels.record, 0))?;
    println!("NEMOPHILA-R0:VALIDATION:PASS");

    let termios = tcgetattr(STDOUT_FILENO)?;
    tcsetattr(STDOUT_FILENO, SetTermiosWhen::Drain, &termios)?;
    set_log_levels(previous)?;
    shutdown(SHUTDOWN_MAGIC)?;
    unreachable!("nemophila-r0-test: shutdown returned unexpectedly")
}
