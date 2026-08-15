use core::mem::size_of;

use anemone_rs::{
    abi::{
        process::linux::{clone::CloneArgs, signal::SIGCHLD},
        syscall::{linux::SYS_CLONE3, syscall},
    },
    os::linux::process::{WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork, gettid, wait4},
    prelude::*,
};

pub(crate) fn run() {
    let creator = gettid().expect("Nemophila Stage 5 failed to read creator TID");
    println!("NEMOPHILA-STAGE5:USER-TEST:START:creator={creator}");

    let clone_child = match fork().expect("Nemophila Stage 5 clone failed") {
        None => exit(0),
        Some(child) => child,
    };
    println!("NEMOPHILA-STAGE5:CLONE:RETURN:creator={creator}:child={clone_child}");
    reap("CLONE", clone_child);

    let clone3_child = raw_clone3().expect("Nemophila Stage 5 clone3 failed");
    if clone3_child == 0 {
        exit(0);
    }
    println!("NEMOPHILA-STAGE5:CLONE3:RETURN:creator={creator}:child={clone3_child}");
    reap("CLONE3", clone3_child);

    println!("NEMOPHILA-STAGE5:USER-TEST:SUMMARY:PASS");
}

fn raw_clone3() -> Result<u32, Errno> {
    let args = CloneArgs {
        flags: 0,
        pidfd: 0,
        child_tid: 0,
        parent_tid: 0,
        exit_signal: SIGCHLD as u64,
        stack: 0,
        stack_size: 0,
        tls: 0,
        set_tid: 0,
        set_tid_size: 0,
        cgroup: 0,
    };
    // This adapter is deliberately local to the focused oracle: the ordinary
    // userspace process API does not grow a clone3 surface for one validation.
    unsafe {
        syscall(
            SYS_CLONE3,
            &args as *const CloneArgs as u64,
            size_of::<CloneArgs>() as u64,
            0,
            0,
            0,
            0,
        )
    }
    .map(|tid| tid as u32)
}

fn reap(label: &str, child: u32) {
    let mut status = WStatusRaw::EMPTY;
    let waited = wait4(
        WaitFor::ChildWithTgid(child),
        Some(&mut status),
        WaitOptions::empty(),
    )
    .unwrap_or_else(|errno| panic!("Nemophila Stage 5 {label} wait4 failed: {errno}"));
    assert_eq!(waited, Some(child), "Nemophila Stage 5 reaped wrong child");
    assert!(
        matches!(status.read(), WStatus::Exited(0)),
        "Nemophila Stage 5 {label} child failed: {:?}",
        status.read()
    );
    println!("NEMOPHILA-STAGE5:{label}:COMPLETE:child={child}");
}
