#![no_std]
#![no_main]

mod udp;
mod unix;

use anemone_rs::{env, prelude::*};

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    let mut args = env::args();
    let _program = args.next();
    match args.next() {
        Some("--udp-cloexec-child") => {
            let fd = args.next().ok_or(EINVAL)?;
            if args.next().is_some() {
                return Err(EINVAL);
            }
            udp::run_cloexec_child(fd)
        },
        None => {
            let udp_result = udp::run();
            let unix_result = unix::run();
            udp_result.and(unix_result)
        },
        Some(_) => Err(EINVAL),
    }
}
