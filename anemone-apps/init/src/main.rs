#![no_std]
#![no_main]

use core::ptr::null_mut;

use anemone_rs::{
    env::*,
    os::linux::{
        fs::{chdir, chroot, mount},
        process::{
            clone, execve, sched_yield, wait4, CloneFlags, WStatusRaw, WaitFor, WaitOptions,
        },
    },
    prelude::*,
    process::process_id,
};

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    let cwd = current_dir()?;
    let pid = process_id();
    println!("init: started:\n\tcwd:{}\n\tpid:{}", cwd.display(), pid);
    let env = envs();
    for (key, value) in env {
        println!("init: env {}={}", key, value);
    }

    // auxv test
    {
        println!("page size: {:#x?}", page_sz());
        println!("random bytes: {:#x?}", random_bytes());
        println!("clock ticks per second: {:#x?}", clktck());
        println!("exec filename: {:#x?}", exec_fn());
        println!("platform: {:#x?}", platform());
        println!("base platform: {:#x?}", base_platform());
    }

    let mut tidc = 0;
    match clone(
        CloneFlags::CHILD_SETTID | CloneFlags::SIGCHLD,
        None,
        None,
        null_mut(),
        Some(&mut tidc),
    )
    .expect("init: failed to clone")
    {
        Some(tid) => {
            println!("init: forked child process with tid {}", tid);
            loop {
                let mut wstatus = WStatusRaw::EMPTY;
                match wait4(WaitFor::AnyChild, Some(&mut wstatus), WaitOptions::empty()) {
                    Ok(Some(tid)) => {
                        println!(
                            "init: child task #{} exited with code {:?}",
                            tid,
                            wstatus.read()
                        )
                    },
                    Ok(None) => {
                        panic!(
                            "init: wait4 returned None but no error, this should not happen, since we didn't specify WNOHANG"
                        );
                    },
                    Err(e) => {
                        if e != ECHILD {
                            panic!("init: cannot recycle child tasks: {}", e);
                        } else {
                            sched_yield().expect("init: failed to yield");
                        }
                    },
                }
            }
        },
        None => {
            // child
            println!("init: in child process, tid: {}", tidc);

            //mount(Some(Path::new("/dev")), target, fstype)
            mount(None, Path::new("/dev"), "devfs").expect("init: failed to mount devfs on /dev");
            mount(Some(Path::new("/dev/vdb")), Path::new("/mnt"), "ext4")
                .expect("init: failed to mount /dev/vdb on /mnt");

            chdir("/mnt").expect("init: failed to change directory to /mnt");
            chroot("/mnt").expect("init: failed to chroot to /mnt");

            // all done.

            chdir("/glibc/basic").expect("init: failed to change directory to /glibc/basic");
            execve("./run-all.sh", &[], &["PATH=/glibc/lib"]).expect("init: failed to execve");

            unreachable!();
        },
    }
}
