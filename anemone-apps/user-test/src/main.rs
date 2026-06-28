#![no_std]
#![no_main]
#![allow(unused)]

mod busybox;
mod competition;
mod file;
mod guest;
mod ltp;
mod process;
mod runtime;

use anemone_rs::{
    abi::system::native::power::SHUTDOWN_MAGIC, os::anemone::power::shutdown, prelude::*,
};

fn local_run_cmd(cmd: &str, args: &[&str], envs: &[&str]) {
    process::run_execve(cmd, args, envs, cmd);
}

/// local tests for development.
fn run_local_tests() {
    // 1. signal test
    // println!("user-test: running signal test...");
    // local_run_cmd("/bin/signal-test", &["signal-test"], &[]);
    // println!("user-test: signal test finished.");

    // 2. float test
    // println!("user-test: running float test...");
    // local_run_cmd("/bin/float-test", &["float-test", "--type", "sig"], &[]);
    // println!("user-test: float test finished.");

    // 3. shm test
    // println!("user-test: running shm test...");
    // local_run_cmd("/bin/shm-test", &["shm-test"], &[]);
    // println!("user-test: shm test finished.");

    // 4. pg test
    // println!("user-test: running pg test...");
    // local_run_cmd("/bin/pg-test", &["pg-test"], &[]);
    // println!("user-test: pg test finished.");

    // 5. mmap test
    // println!("user-test: running mmap test...");
    // local_run_cmd("/bin/mmap-test", &["mmap-test"], &[]);
    // println!("user-test: mmap test finished.");

    // 6. OOM killer test
    // println!("user-test: running OOM killer test...");
    // local_run_cmd("/bin/oom-killer-test", &["oom-killer-test"], &[]);
    // println!("user-test: OOM killer test finished.");

    // 7. pthread create serial1 stress test
    // println!("user-test: running pthread create stress test...");
    // local_run_cmd(
    //     "/bin/pthread-create-stress",
    //     &["pthread-create-stress"],
    //     &[],
    // );
    // println!("user-test: pthread create stress test finished.");
}

/// competition tests.
fn run_comp_tests() {
    guest::enter_competition_root();
    guest::init_competition_environment();
    ltp::install_ltp_fixtures();

    competition::run_competition_tests();
    // ltp::run_ltp_tests();

    println!("user-test: all competition tests finished.");
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    run_local_tests();

    run_comp_tests();

    println!("user-test: all tests finished, shutting down.");
    shutdown(SHUTDOWN_MAGIC).expect("user-test: failed to request shutdown");
    unreachable!("user-test: shutdown returned unexpectedly");
}
