#![no_std]
#![no_main]

use core::{
    ptr::null_mut,
    sync::atomic::{AtomicUsize, Ordering},
};

use anemone_rs::{
    env,
    os::linux::process::{
        self, mmap, sched_yield, spawn_raw_thread, CloneFlags, MmapFlags, MmapProt, Tid,
    },
    prelude::*,
};

const PAGE_SIZE: usize = 4096;
const DEFAULT_STACK_SIZE: usize = 16 * 1024;
const WAIT_RETRIES_PER_THREAD: usize = 4096;

#[cfg(target_arch = "loongarch64")]
const DEFAULT_THREAD_COUNT: usize = 10;
#[cfg(not(target_arch = "loongarch64"))]
const DEFAULT_THREAD_COUNT: usize = 2500;

static EXITED_THREADS: AtomicUsize = AtomicUsize::new(0);

struct Config {
    count: usize,
    stack_size: usize,
    drain: bool,
}

impl Config {
    fn default() -> Self {
        Self {
            count: DEFAULT_THREAD_COUNT,
            stack_size: DEFAULT_STACK_SIZE,
            drain: true,
        }
    }
}

extern "C" fn empty_thread(_: usize) -> ! {
    EXITED_THREADS.fetch_add(1, Ordering::SeqCst);
    process::exit(0)
}

fn usage() {
    println!("usage: pthread-create-stress [--count N] [--stack-size BYTES] [--no-drain]");
}

fn parse_usize(value: Option<&str>, name: &str) -> usize {
    value
        .unwrap_or_else(|| panic!("pthread-create-stress: missing value for {name}"))
        .parse::<usize>()
        .unwrap_or_else(|_| panic!("pthread-create-stress: invalid integer for {name}"))
}

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

fn parse_config() -> Result<Option<Config>, Errno> {
    let mut config = Config::default();
    let mut args = env::args();
    let _ = args.next();

    while let Some(arg) = args.next() {
        match arg {
            "--help" | "-h" => {
                usage();
                return Ok(None);
            },
            "--count" => {
                config.count = parse_usize(args.next(), "--count");
            },
            "--stack-size" => {
                config.stack_size = parse_usize(args.next(), "--stack-size");
            },
            "--no-drain" => {
                config.drain = false;
            },
            _ => {
                usage();
                panic!("pthread-create-stress: unknown argument {arg}");
            },
        }
    }

    if config.count == 0 || config.stack_size == 0 {
        return Err(EINVAL);
    }
    config.stack_size = align_up(config.stack_size, PAGE_SIZE);
    Ok(Some(config))
}

fn wait_for_thread_exits(target_exited: usize, batch_count: usize) -> Result<(), Errno> {
    for _ in 0..batch_count.saturating_mul(WAIT_RETRIES_PER_THREAD) {
        let exited = EXITED_THREADS.load(Ordering::SeqCst);
        if exited >= target_exited {
            return Ok(());
        }
        sched_yield()?;
    }

    panic!(
        "pthread-create-stress: timed out waiting for raw threads to exit: expected at least {}, got {}",
        target_exited,
        EXITED_THREADS.load(Ordering::SeqCst)
    );
}

fn spawn_empty_thread(stack_size: usize, child_tid: &mut Tid) -> Result<Tid, Errno> {
    let stack = mmap(
        0,
        stack_size,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?;
    let stack_top = unsafe { stack.as_ptr().add(stack_size) };
    let mut parent_tid = 0;
    let flags = CloneFlags::VM
        | CloneFlags::FS
        | CloneFlags::FILES
        | CloneFlags::SIGHAND
        | CloneFlags::THREAD
        | CloneFlags::SYSVSEM
        | CloneFlags::PARENT_SETTID
        | CloneFlags::CHILD_SETTID
        | CloneFlags::CHILD_CLEARTID;

    // This intentionally mirrors libc-bench b_pthread_create_serial1's
    // create-only pressure: child stacks and child_tid slots remain live until
    // process exit, and no pthread-style join/reap is performed.
    unsafe {
        spawn_raw_thread(
            flags,
            stack_top,
            Some(&mut parent_tid),
            null_mut(),
            Some(child_tid),
            empty_thread,
            0,
        )
    }
}

fn run_create_serial1(config: Config) -> Result<(), Errno> {
    println!(
        "pthread-create-stress: create-serial1 count={} stack_size={} drain={}",
        config.count, config.stack_size, config.drain
    );

    let start_exited = EXITED_THREADS.load(Ordering::SeqCst);
    let target_exited = start_exited.saturating_add(config.count);
    let mut child_tids = vec![0; config.count].into_boxed_slice();
    for i in 0..config.count {
        let tid = spawn_empty_thread(config.stack_size, &mut child_tids[i])?;
        if (i + 1) % 256 == 0 || i + 1 == config.count {
            println!("pthread-create-stress: created {} last_tid={}", i + 1, tid);
        }
    }

    if config.drain {
        wait_for_thread_exits(target_exited, config.count)?;
    }

    println!(
        "pthread-create-stress: ok created={} exited={}",
        config.count,
        EXITED_THREADS.load(Ordering::SeqCst)
    );
    Ok(())
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    const LOOP_COUNT: usize = 10;

    for i in 0..LOOP_COUNT {
        println!("pthread-create-stress: loop {}/{}", i + 1, LOOP_COUNT);
        if let Some(config) = parse_config()? {
            run_create_serial1(config)?;
        }
    }
    Ok(())
}
