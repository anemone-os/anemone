#![no_std]
#![no_main]

use core::ptr::null_mut;

use anemone_rs::{
    env::current_dir,
    os::linux::process::{clone, execve, getpid, CloneFlags},
    prelude::*,
};

#[main]
pub fn main() -> Result<(), anemone_abi::errno::Errno> {
    let cwd = current_dir()?;
    let pid = getpid()?;
    println!("init: started:\n\tcwd:{}\n\tpid:{}", cwd.display(), pid);

    let mut tidp = 0;
    let mut tidc = 0;
    clone(
        CloneFlags::CLONE_PARENT_SETTID | CloneFlags::CLONE_CHILD_SETTID,
        None,
        &mut tidp,
        null_mut(),
        &mut tidc,
    )
    .unwrap();
    if tidp == 0 {
        println!("init: get into cloned task {}", tidc);
        execve("bin/user-test", &["bin/user-test", "1"]).expect("failed to execve user-test");
    } else {
        println!("init: 'bin/user-test' started with pid {}", tidp);
    }
    loop {}
}
