#![no_std]
#![no_main]
#![allow(unused)]

mod busybox;
mod clock_read;
mod clock_step;
mod competition;
mod file;
mod guest;
mod ltp;
mod process;
mod runtime;
mod soft_timer;
mod timer_signal;

use anemone_rs::{
    abi::{fs::linux::open::O_RDONLY, system::native::power::SHUTDOWN_MAGIC},
    os::{
        anemone::power::shutdown,
        linux::fs::{self, AtFd},
    },
    prelude::*,
};

fn local_run_cmd(cmd: &str, args: &[&str], envs: &[&str]) {
    process::run_execve(cmd, args, envs, cmd);
}

fn path_exists(path: &str) -> bool {
    let Ok(fd) = fs::openat(AtFd::Cwd, Path::new(path), O_RDONLY, 0) else {
        return false;
    };
    let _ = fs::close(fd);
    true
}

fn run_udp_extension_c_consumer() {
    if !path_exists("/bin/udp-extension-c") {
        return;
    }
    println!("user-test: running UDP extension C consumer local matrix...");
    local_run_cmd("/bin/udp-extension-c", &["udp-extension-c", "--local"], &[]);
    println!("user-test: UDP extension C consumer local matrix finished.");
}

/// local tests for development.
fn run_local_tests() {
    run_udp_extension_c_consumer();

    println!("user-test: running local POSIX timer test...");
    local_run_cmd("/bin/local-test", &["local-test"], &[]);
    println!("user-test: local POSIX timer test finished.");

    // println!("user-test: running native clock read test...");
    // clock_read::verify_native_clocks();
    // println!("user-test: native clock read test finished.");
    //
    // println!("user-test: running soft timer consumer test...");
    // soft_timer::verify_soft_timer_consumers();
    // println!("user-test: soft timer consumer test finished.");
    //
    // println!("user-test: running realtime clock step test...");
    // clock_step::verify_clock_steps();
    // println!("user-test: realtime clock step test finished.");
    //
    // println!("user-test: running SI_TIMER signal frame test...");
    // timer_signal::verify_timer_signal_frame();
    // println!("user-test: SI_TIMER signal frame test finished.");
    //
    // println!("user-test: running userptr test...");
    // local_run_cmd("/bin/userptr", &["userptr"], &[]);
    // println!("user-test: userptr test finished.");

    // 0. ioctl test
    // println!("user-test: running ioctl test...");
    // local_run_cmd("/bin/ioctl-test", &["ioctl-test"], &[]);
    // println!("user-test: ioctl test finished.");

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

    // 8. fair-test
    // println!("user-test: running fair test...");
    // local_run_cmd("/bin/fair-test", &["fair-test"], &[]);
    // println!("user-test: fair test finished.");

    // 9. sched dynamic attributes focused test
    // println!("user-test: running sched attr test...");
    // local_run_cmd("/bin/sched-attr-test", &["sched-attr-test"], &[]);
    // println!("user-test: sched attr test finished.");

    // 10. job control test
    // #[cfg(target_arch = "riscv64")]
    // {
    //     println!("user-test: running jobctl test...");
    //     local_run_cmd("/bin/jobctl-test", &["jobctl-test"], &[]);
    //     println!("user-test: jobctl test finished.");
    // }

    // 11. flock test
    // println!("user-test: running flock test...");
    // local_run_cmd("/bin/flock-test", &["flock-test"], &[]);
    // println!("user-test: flock test finished.");

    // 12. epoll test
    // println!("user-test: running epoll test...");
    // local_run_cmd("/bin/epoll-test", &["epoll-test"], &[]);
    // println!("user-test: epoll test finished.");

    // 13. Socket suites: UDP regression and AF_UNIX Stage 1 vertical slice
    println!("user-test: running socket test...");
    local_run_cmd("/bin/socket-test", &["socket-test"], &[]);
    println!("user-test: socket test finished.");

    println!("user-test: running Rust Command seqpacket consumer...");
    local_run_cmd("/bin/rust-command-test", &["rust-command-test"], &[]);
    println!("user-test: Rust Command seqpacket consumer finished.");

    println!("user-test: running pipe capacity test...");
    local_run_cmd("/bin/fcntl-test", &["fcntl-test", "pipe-capacity"], &[]);
    println!("user-test: pipe capacity test finished.");

    // println!("user-test: running POSIX record lock test...");
    // local_run_cmd(
    //     "/bin/fcntl-test",
    //     &["fcntl-test", "posix-record-lock"],
    //     &[],
    // );
    // println!("user-test: POSIX record lock test finished.");
}

/// competition tests.
fn run_comp_tests() {
    guest::enter_competition_root();
    guest::init_competition_environment();

    println!("user-test: running BusyBox loopback ping...");
    local_run_cmd("/bin/ping", &["ping", "-c", "1", "127.0.0.1"], &[]);
    println!("user-test: BusyBox loopback ping finished.");

    println!("user-test: running BusyBox gateway ping...");
    local_run_cmd("/bin/ping", &["ping", "-c", "1", "10.0.2.2"], &[]);
    println!("user-test: BusyBox gateway ping finished.");

    ltp::install_ltp_fixtures();

    // competition::run_competition_tests();
    ltp::run_ltp_tests();

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
