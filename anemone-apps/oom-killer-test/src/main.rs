#![no_std]
#![no_main]

use core::ptr::{null_mut, write_volatile};

use anemone_rs::{
    abi::process::linux::signal::{SIGCHLD, SIGKILL},
    os::linux::process::{
        CloneFlags, MmapFlags, MmapProt, WStatus, WStatusRaw, WaitFor, WaitOptions, clone, exit,
        mmap, wait4,
    },
    prelude::*,
};

const PAGE_SIZE: usize = 4096;
const CHUNK_SIZE: usize = 50 * 1024 * 1024;

fn touch_chunk(ptr: *mut u8, chunk_idx: usize) {
    let value = chunk_idx as u8;
    for offset in (0..CHUNK_SIZE).step_by(PAGE_SIZE) {
        unsafe {
            write_volatile(ptr.add(offset), value);
        }
    }
}

fn child_allocate_forever() -> ! {
    let mut chunk_idx = 0usize;

    loop {
        let mapping = match mmap(
            0,
            CHUNK_SIZE,
            MmapProt::PROT_READ | MmapProt::PROT_WRITE,
            MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
            None,
            None,
        ) {
            Ok(mapping) => mapping,
            Err(errno) => {
                println!("oom-killer-test: child mmap failed before SIGKILL: {errno:?}");
                exit(2);
            },
        };

        touch_chunk(mapping.as_ptr(), chunk_idx);
        chunk_idx += 1;
        println!(
            "oom-killer-test: child materialized {} MiB",
            chunk_idx * CHUNK_SIZE / 1024 / 1024
        );
    }
}

fn wait_for_oom_kill(child: u32) -> Result<(), Errno> {
    loop {
        let mut wstatus = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(child),
            Some(&mut wstatus),
            WaitOptions::empty(),
        ) {
            Ok(Some(waited)) => {
                assert_eq!(waited, child, "oom-killer-test: waited pid mismatch");
                match wstatus.read() {
                    WStatus::Signal(sig) if sig == SIGKILL as i8 => return Ok(()),
                    other => panic!("oom-killer-test: child was not OOM-killed: {other:?}"),
                }
            },
            Ok(None) => panic!("oom-killer-test: wait4 returned None without WNOHANG"),
            Err(EINTR) => continue,
            Err(errno) => return Err(errno),
        }
    }
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    println!("oom-killer-test: starting child memory pressure");

    match clone(
        CloneFlags::empty(),
        Some(SIGCHLD),
        None,
        None,
        null_mut(),
        None,
    )? {
        Some(child) => {
            wait_for_oom_kill(child)?;
            println!("oom-killer-test: PASS child was killed by SIGKILL");
            Ok(())
        },
        None => child_allocate_forever(),
    }
}
