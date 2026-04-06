#![no_std]
#![no_main]

use anemone_rs::{
    fs::getcwd,
    println,
    process::{clone, execve, getpid},
};

#[anemone_rs::main]
pub fn main() -> Result<(), anemone_abi::errno::Errno> {
    let cwd = getcwd().unwrap();
    let pid = getpid().unwrap();
    println!("init: started:\n\tcwd:{}\n\tpid:{}", cwd, pid);
    let mut tidp = 0;
    let mut tidc = 0;
    clone(&mut tidp, &mut tidc).unwrap();
    if tidp == 0 {
        println!("init: get into cloned task {}", tidc);
        execve("bin/user-test", &["bin/user-test", "1"]).expect("failed to execve user-test");
    } else {
        println!("init: 'bin/user-test' started with pid {}", tidp);
    }
    loop {}
}
