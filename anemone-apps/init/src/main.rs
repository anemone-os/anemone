#![no_std]
#![no_main]

use anemone_rs::process::execve;

#[anemone_rs::main]
pub fn main() -> Result<(), anemone_abi::errno::Errno> {
    execve("bin/user-test", &["bin/user-test", "1"]).expect("failed to execve user-test");
    Ok(())
}
