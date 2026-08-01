#![no_std]
#![no_main]

mod nofile;

use anemone_rs::{
    abi::system::native::power::SHUTDOWN_MAGIC, env, os::anemone::power::shutdown, prelude::*,
    process::process_id,
};

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    let mut args = env::args();
    let _program = args.next();
    match args.next() {
        Some("nofile") if args.next().is_none() => nofile::run(),
        Some("--exec-nofile") => nofile::exec_child(args.next(), args.next()),
        None => {
            nofile::run()?;
            if process_id() == 1 {
                shutdown(SHUTDOWN_MAGIC)?;
                unreachable!("rlimit-test: shutdown returned unexpectedly");
            }
            Err(EINVAL)
        },
        _ => Err(EINVAL),
    }
}
