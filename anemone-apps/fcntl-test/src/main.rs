#![no_std]
#![no_main]

mod posix_record_lock;

use anemone_rs::{env, prelude::*};

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    let mut args = env::args();
    let _ = args.next();
    let suite = args.next().ok_or(EINVAL)?;
    if args.next().is_some() {
        return Err(EINVAL);
    }

    match suite {
        "posix-record-lock" => posix_record_lock::run(),
        _ => Err(EINVAL),
    }
}
