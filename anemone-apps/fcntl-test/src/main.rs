#![no_std]
#![no_main]

mod posix_record_lock;

use anemone_rs::{env, prelude::*};

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    let mut args = env::args();
    let _ = args.next();
    let suite = args.next().ok_or(EINVAL)?;

    match suite {
        "posix-record-lock" if args.next().is_none() => posix_record_lock::run(),
        mode if mode.starts_with("--posix-exec-") => {
            let result = posix_record_lock::exec_child(
                mode,
                args.next(),
                args.next(),
                args.next(),
                args.next(),
            );
            if args.next().is_some() {
                Err(EINVAL)
            } else {
                result
            }
        },
        _ => Err(EINVAL),
    }
}
