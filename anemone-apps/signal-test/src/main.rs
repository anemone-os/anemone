#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering};

#[cfg(target_arch = "riscv64")]
use core::arch::naked_asm;

use anemone_rs::{
    abi::process::linux::{
        signal::{self as linux_signal, SigInfo},
        ucontext::UContext,
    },
    os::linux::process::{
        self, CloneFlags, MmapFlags, MmapProt, WStatus, WStatusRaw, WaitFor, WaitOptions, getpid,
        gettid, mmap, sched_yield,
        signal::{self, SigNo, SigProcMaskHow},
        wait4,
    },
    prelude::*,
};

const WAIT_RETRIES: usize = 100_000;
const THREAD_STACK_SIZE: usize = 64 * 1024;
const PROCESS_SIGVAL: usize = 0x1357_2468;
const SELF_SIGNAL_STRESS_ROUNDS: usize = 64;
const THREAD_SIGNAL_STRESS_ROUNDS: usize = 64;
const RT_QUEUE_STRESS_COUNT: usize = 32;
const RT_QUEUE_SIGVAL_BASE: usize = 0x5000;

static SIMPLE_COUNT: AtomicUsize = AtomicUsize::new(0);
static SIGINFO_COUNT: AtomicUsize = AtomicUsize::new(0);
static LAST_SIGINFO_SIGNO: AtomicUsize = AtomicUsize::new(0);
static LAST_UCONTEXT_PC: AtomicUsize = AtomicUsize::new(0);
static LAST_SIMPLE_HANDLER_TID: AtomicUsize = AtomicUsize::new(0);
static PROCESS_SHARED_STATE_PTR: AtomicUsize = AtomicUsize::new(0);
static RT_QUEUE_SHARED_STATE_PTR: AtomicUsize = AtomicUsize::new(0);
static THREAD_READY: AtomicUsize = AtomicUsize::new(0);
static THREAD_TID: AtomicUsize = AtomicUsize::new(0);
static THREAD_DONE: AtomicUsize = AtomicUsize::new(0);
static THREAD_EXITED: AtomicUsize = AtomicUsize::new(0);

#[repr(C)]
struct ProcessSignalSharedState {
    child_ready: AtomicUsize,
    child_tid: AtomicUsize,
    handler_count: AtomicUsize,
    handled_tid: AtomicUsize,
    handled_signo: AtomicUsize,
    handled_sigval: AtomicUsize,
    child_done: AtomicUsize,
}

impl ProcessSignalSharedState {
    const fn new() -> Self {
        Self {
            child_ready: AtomicUsize::new(0),
            child_tid: AtomicUsize::new(0),
            handler_count: AtomicUsize::new(0),
            handled_tid: AtomicUsize::new(0),
            handled_signo: AtomicUsize::new(0),
            handled_sigval: AtomicUsize::new(0),
            child_done: AtomicUsize::new(0),
        }
    }
}

#[repr(C)]
struct RtQueueSharedState {
    child_ready: AtomicUsize,
    child_tid: AtomicUsize,
    received_count: AtomicUsize,
    last_tid: AtomicUsize,
    overflow_count: AtomicUsize,
    child_done: AtomicUsize,
    observed_sigvals: [AtomicUsize; RT_QUEUE_STRESS_COUNT],
}

impl RtQueueSharedState {
    const fn new() -> Self {
        Self {
            child_ready: AtomicUsize::new(0),
            child_tid: AtomicUsize::new(0),
            received_count: AtomicUsize::new(0),
            last_tid: AtomicUsize::new(0),
            overflow_count: AtomicUsize::new(0),
            child_done: AtomicUsize::new(0),
            observed_sigvals: [const { AtomicUsize::new(0) }; RT_QUEUE_STRESS_COUNT],
        }
    }
}

fn process_shared_state() -> Option<&'static ProcessSignalSharedState> {
    let ptr = PROCESS_SHARED_STATE_PTR.load(Ordering::SeqCst) as *const ProcessSignalSharedState;
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

fn rt_queue_shared_state() -> Option<&'static RtQueueSharedState> {
    let ptr = RT_QUEUE_SHARED_STATE_PTR.load(Ordering::SeqCst) as *const RtQueueSharedState;
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

fn wait_until<F>(mut pred: F, what: &str) -> Result<(), Errno>
where
    F: FnMut() -> bool,
{
    for _ in 0..WAIT_RETRIES {
        if pred() {
            return Ok(());
        }
        sched_yield()?;
    }
    panic!("signal-test: timed out waiting for {what}");
}

fn wait_for_atomic(atom: &AtomicUsize, expected: usize, what: &str) -> Result<(), Errno> {
    wait_until(|| atom.load(Ordering::SeqCst) == expected, what)
}

#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
unsafe extern "C" fn raw_clone_thread(flags: u64, stack_top: u64, entry: usize, arg: usize) -> i64 {
    naked_asm!(
        "mv t0, a2",
        "mv t1, a3",
        "mv a2, zero",
        "mv a3, zero",
        "mv a4, zero",
        "li a7, {sys_clone}",
        "ecall",
        "bnez a0, 2f",
        "mv a0, t1",
        "jalr t0",
        "2:",
        "ret",
        sys_clone = const anemone_rs::abi::syscall::linux::SYS_CLONE,
    )
}

#[cfg(target_arch = "riscv64")]
unsafe fn spawn_thread(
    flags: CloneFlags,
    stack_top: *mut u8,
    entry: extern "C" fn(usize) -> !,
    arg: usize,
) -> Result<u32, Errno> {
    let ret =
        unsafe { raw_clone_thread(flags.bits() as u64, stack_top as u64, entry as usize, arg) };
    if ret < 0 {
        Err((-ret) as i32)
    } else {
        Ok(ret as u32)
    }
}

#[cfg(not(target_arch = "riscv64"))]
unsafe fn spawn_thread(
    _flags: CloneFlags,
    _stack_top: *mut u8,
    _entry: extern "C" fn(usize) -> !,
    _arg: usize,
) -> Result<u32, Errno> {
    Err(ENOSYS)
}

extern "C" fn thread_child_entry(_: usize) -> ! {
    THREAD_TID.store(
        gettid().expect("signal-test: gettid failed in thread child") as usize,
        Ordering::SeqCst,
    );
    THREAD_READY.store(1, Ordering::SeqCst);
    wait_for_atomic(&THREAD_DONE, 1, "thread completion")
        .expect("signal-test: thread child wait failed");
    THREAD_EXITED.store(1, Ordering::SeqCst);
    process::exit(0)
}

#[anemone_rs::signal_handler]
fn simple_handler(signo: SigNo) {
    assert_eq!(signo.as_usize(), SigNo::SIGUSR1.as_usize());
    LAST_SIMPLE_HANDLER_TID.store(
        gettid().expect("signal-test: gettid failed in simple handler") as usize,
        Ordering::SeqCst,
    );
    SIMPLE_COUNT.fetch_add(1, Ordering::SeqCst);
}

#[anemone_rs::signal_handler(siginfo)]
fn siginfo_handler(signo: SigNo, siginfo: *const SigInfo, ucontext: *const UContext) {
    assert_eq!(signo.as_usize(), SigNo::SIGUSR2.as_usize());
    assert!(!siginfo.is_null());
    assert!(!ucontext.is_null());

    let siginfo = unsafe { &*siginfo };
    let ucontext = unsafe { &*ucontext };
    assert_eq!(siginfo.si_signo as usize, SigNo::SIGUSR2.as_usize());
    assert_ne!(ucontext.uc_mcontext.pc(), 0);

    LAST_SIGINFO_SIGNO.store(siginfo.si_signo as usize, Ordering::SeqCst);
    LAST_UCONTEXT_PC.store(ucontext.uc_mcontext.pc() as usize, Ordering::SeqCst);
    SIGINFO_COUNT.fetch_add(1, Ordering::SeqCst);

    if let Some(shared) = process_shared_state() {
        shared.handler_count.fetch_add(1, Ordering::SeqCst);
        shared.handled_tid.store(
            gettid().expect("signal-test: gettid failed in siginfo handler") as usize,
            Ordering::SeqCst,
        );
        shared
            .handled_signo
            .store(siginfo.si_signo as usize, Ordering::SeqCst);
        if siginfo.si_code == linux_signal::SI_QUEUE {
            let sigval = unsafe { siginfo.fields.rt.sigval.as_u64() } as usize;
            shared.handled_sigval.store(sigval, Ordering::SeqCst);
        }
    }
}

#[anemone_rs::signal_handler(siginfo)]
fn rt_queue_handler(signo: SigNo, siginfo: *const SigInfo, _ucontext: *const UContext) {
    assert_eq!(signo.as_usize(), linux_signal::SIGRTMIN as usize);
    assert!(!siginfo.is_null());

    let siginfo = unsafe { &*siginfo };
    assert_eq!(siginfo.si_signo as usize, linux_signal::SIGRTMIN as usize);
    assert_eq!(siginfo.si_code, linux_signal::SI_QUEUE);

    if let Some(shared) = rt_queue_shared_state() {
        let slot = shared.received_count.fetch_add(1, Ordering::SeqCst);
        let sigval = unsafe { siginfo.fields.rt.sigval.as_u64() } as usize;
        if slot < RT_QUEUE_STRESS_COUNT {
            shared.observed_sigvals[slot].store(sigval, Ordering::SeqCst);
        } else {
            shared.overflow_count.fetch_add(1, Ordering::SeqCst);
        }
        shared.last_tid.store(
            gettid().expect("signal-test: gettid failed in rt queue handler") as usize,
            Ordering::SeqCst,
        );
    }
}

fn install_handler(sig: SigNo, handler: *const (), sa_flags: u64) -> Result<(), Errno> {
    let action = linux_signal::SigAction {
        sighandler: handler,
        sa_flags,
        sa_mask: linux_signal::SigSet { bits: 0 },
    };
    signal::sigaction(sig, Some(&action), None)
}

fn sigset_of(sig: SigNo) -> linux_signal::SigSet {
    linux_signal::SigSet {
        bits: 1u64 << sig.as_usize(),
    }
}

fn queue_siginfo(sig: SigNo, sigval: usize) -> linux_signal::SigInfoWrapper {
    linux_signal::SigInfoWrapper {
        info: linux_signal::SigInfo {
            si_signo: sig.as_usize() as i32,
            si_errno: 0,
            si_code: linux_signal::SI_QUEUE,
            fields: linux_signal::sifields::SigInfoFields {
                rt: linux_signal::sifields::Rt {
                    pid: 0,
                    uid: 0,
                    sigval: linux_signal::sifields::SigVal {
                        sival_int: sigval as i32,
                    },
                },
            },
        },
    }
}

fn rt_queue_sig() -> SigNo {
    SigNo::new(linux_signal::SIGRTMIN as usize)
}

fn run_self_signal_stress_test(main_tid: u32) -> Result<(), Errno> {
    println!("signal-test: self signal stress");

    let mut expected = SIMPLE_COUNT.load(Ordering::SeqCst);
    for _ in 0..SELF_SIGNAL_STRESS_ROUNDS {
        signal::raise(SigNo::SIGUSR1)?;
        expected += 1;
        wait_until(
            || {
                SIMPLE_COUNT.load(Ordering::SeqCst) == expected
                    && LAST_SIMPLE_HANDLER_TID.load(Ordering::SeqCst) == main_tid as usize
            },
            "self-directed signal stress delivery",
        )?;
    }

    Ok(())
}

fn wait_child_exit_ok(pid: u32, name: &str) -> Result<(), Errno> {
    loop {
        let mut wstatus = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut wstatus),
            WaitOptions::empty(),
        ) {
            Ok(Some(waited)) => {
                assert_eq!(waited, pid, "signal-test: {name} waited pid mismatch");
                match wstatus.read() {
                    WStatus::Exited(0) => return Ok(()),
                    other => panic!("signal-test: {name} child exited unexpectedly: {other:?}"),
                }
            },
            Ok(None) => {
                panic!("signal-test: {name} wait4 returned None without WNOHANG");
            },
            Err(EINTR) => continue,
            Err(errno) => return Err(errno),
        }
    }
}

fn run_cross_thread_tgkill_test(main_tgid: u32) -> Result<(), Errno> {
    println!("signal-test: cross-thread tgkill");

    THREAD_READY.store(0, Ordering::SeqCst);
    THREAD_TID.store(0, Ordering::SeqCst);
    THREAD_DONE.store(0, Ordering::SeqCst);
    THREAD_EXITED.store(0, Ordering::SeqCst);

    let stack = mmap(
        0,
        THREAD_STACK_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?;
    let stack_top = unsafe { stack.as_ptr().add(THREAD_STACK_SIZE) };

    let thread_tid = unsafe {
        spawn_thread(
            CloneFlags::VM
                | CloneFlags::FS
                | CloneFlags::FILES
                | CloneFlags::SIGHAND
                | CloneFlags::THREAD,
            stack_top,
            thread_child_entry,
            0,
        )?
    };

    wait_for_atomic(&THREAD_READY, 1, "thread ready")?;
    assert_eq!(THREAD_TID.load(Ordering::SeqCst) as u32, thread_tid);

    let mut expected = SIMPLE_COUNT.load(Ordering::SeqCst);
    for _ in 0..THREAD_SIGNAL_STRESS_ROUNDS {
        signal::tgkill(main_tgid, thread_tid, SigNo::SIGUSR1)?;
        expected += 1;
        wait_until(
            || {
                SIMPLE_COUNT.load(Ordering::SeqCst) == expected
                    && LAST_SIMPLE_HANDLER_TID.load(Ordering::SeqCst) == thread_tid as usize
            },
            "thread-targeted signal delivery",
        )?;
    }

    THREAD_DONE.store(1, Ordering::SeqCst);
    wait_for_atomic(&THREAD_EXITED, 1, "thread exit")?;
    Ok(())
}

fn run_cross_process_sigqueueinfo_test() -> Result<(), Errno> {
    println!("signal-test: cross-process sigqueueinfo");

    let shared_mapping = mmap(
        0,
        size_of::<ProcessSignalSharedState>(),
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_SHARED | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?;
    let shared_ptr = shared_mapping.as_ptr().cast::<ProcessSignalSharedState>();
    unsafe {
        shared_ptr.write(ProcessSignalSharedState::new());
    }
    PROCESS_SHARED_STATE_PTR.store(shared_ptr as usize, Ordering::SeqCst);

    let child_pid = match process::fork()? {
        Some(pid) => pid,
        None => {
            let shared =
                process_shared_state().expect("signal-test: shared state missing in child");
            let tid = gettid()? as usize;
            shared.child_tid.store(tid, Ordering::SeqCst);
            shared.child_ready.store(1, Ordering::SeqCst);
            wait_until(
                || shared.handler_count.load(Ordering::SeqCst) == 1,
                "queued signal delivery in child",
            )?;
            shared.child_done.store(1, Ordering::SeqCst);
            process::exit(0);
        },
    };

    let shared = process_shared_state().expect("signal-test: shared state missing in parent");
    wait_for_atomic(&shared.child_ready, 1, "child process ready")?;

    let child_tid = shared.child_tid.load(Ordering::SeqCst) as u32;
    assert_eq!(
        child_tid, child_pid,
        "signal-test: expected single-thread child pid==tid"
    );

    let info = queue_siginfo(SigNo::SIGUSR2, PROCESS_SIGVAL);
    signal::sigqueueinfo(child_pid, SigNo::SIGUSR2, &info)?;

    wait_until(
        || {
            shared.handler_count.load(Ordering::SeqCst) == 1
                && shared.handled_tid.load(Ordering::SeqCst) == child_tid as usize
                && shared.handled_signo.load(Ordering::SeqCst) == SigNo::SIGUSR2.as_usize()
                && shared.handled_sigval.load(Ordering::SeqCst) == PROCESS_SIGVAL
        },
        "process-directed queued signal delivery",
    )?;
    wait_for_atomic(&shared.child_done, 1, "child process completion")?;
    wait_child_exit_ok(child_pid, "cross-process signal")?;

    PROCESS_SHARED_STATE_PTR.store(0, Ordering::SeqCst);
    process::munmap(
        shared_mapping.as_ptr(),
        size_of::<ProcessSignalSharedState>(),
    )?;
    Ok(())
}

fn run_realtime_sigqueue_stress_test() -> Result<(), Errno> {
    println!("signal-test: realtime sigqueue stress");

    let shared_mapping = mmap(
        0,
        size_of::<RtQueueSharedState>(),
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_SHARED | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?;
    let shared_ptr = shared_mapping.as_ptr().cast::<RtQueueSharedState>();
    unsafe {
        shared_ptr.write(RtQueueSharedState::new());
    }
    RT_QUEUE_SHARED_STATE_PTR.store(shared_ptr as usize, Ordering::SeqCst);

    let child_pid = match process::fork()? {
        Some(pid) => pid,
        None => {
            let shared = rt_queue_shared_state()
                .expect("signal-test: rt queue shared state missing in child");
            let tid = gettid()? as usize;
            shared.child_tid.store(tid, Ordering::SeqCst);
            shared.child_ready.store(1, Ordering::SeqCst);
            wait_until(
                || shared.received_count.load(Ordering::SeqCst) >= RT_QUEUE_STRESS_COUNT,
                "realtime queued signal delivery in child",
            )?;
            shared.child_done.store(1, Ordering::SeqCst);
            process::exit(0);
        },
    };

    let shared =
        rt_queue_shared_state().expect("signal-test: rt queue shared state missing in parent");
    wait_for_atomic(&shared.child_ready, 1, "rt queue child ready")?;

    let child_tid = shared.child_tid.load(Ordering::SeqCst) as u32;
    assert_eq!(
        child_tid, child_pid,
        "signal-test: expected single-thread rt queue child pid==tid"
    );

    let rt_sig = rt_queue_sig();
    for idx in 0..RT_QUEUE_STRESS_COUNT {
        let payload = RT_QUEUE_SIGVAL_BASE + idx;
        let info = queue_siginfo(rt_sig, payload);
        signal::sigqueueinfo(child_pid, rt_sig, &info)?;
    }

    wait_for_atomic(&shared.child_done, 1, "rt queue child completion")?;
    wait_child_exit_ok(child_pid, "realtime sigqueue stress")?;

    assert_eq!(
        shared.received_count.load(Ordering::SeqCst),
        RT_QUEUE_STRESS_COUNT,
        "signal-test: unexpected realtime queued signal count"
    );
    assert_eq!(
        shared.overflow_count.load(Ordering::SeqCst),
        0,
        "signal-test: realtime queue overflow observed"
    );
    assert_eq!(
        shared.last_tid.load(Ordering::SeqCst),
        child_tid as usize,
        "signal-test: realtime queue handler ran on wrong tid"
    );
    for idx in 0..RT_QUEUE_STRESS_COUNT {
        assert_eq!(
            shared.observed_sigvals[idx].load(Ordering::SeqCst),
            RT_QUEUE_SIGVAL_BASE + idx,
            "signal-test: realtime queued payload order mismatch at slot {}",
            idx,
        );
    }

    RT_QUEUE_SHARED_STATE_PTR.store(0, Ordering::SeqCst);
    process::munmap(shared_mapping.as_ptr(), size_of::<RtQueueSharedState>())?;
    Ok(())
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    let main_tid = gettid()?;
    let main_tgid = getpid()?;

    println!("signal-test: installing handlers...");

    install_handler(SigNo::SIGUSR1, simple_handler as *const (), 0)?;
    install_handler(
        SigNo::SIGUSR2,
        siginfo_handler as *const (),
        linux_signal::SA_SIGINFO,
    )?;
    install_handler(
        rt_queue_sig(),
        rt_queue_handler as *const (),
        linux_signal::SA_SIGINFO,
    )?;

    println!("signal-test: raising SIGUSR1");
    signal::raise(SigNo::SIGUSR1)?;
    assert_eq!(SIMPLE_COUNT.load(Ordering::SeqCst), 1);
    assert_eq!(
        LAST_SIMPLE_HANDLER_TID.load(Ordering::SeqCst),
        main_tid as usize
    );

    println!("signal-test: raising SIGUSR2 with siginfo");
    signal::raise(SigNo::SIGUSR2)?;
    assert_eq!(SIGINFO_COUNT.load(Ordering::SeqCst), 1);
    assert_eq!(
        LAST_SIGINFO_SIGNO.load(Ordering::SeqCst),
        SigNo::SIGUSR2.as_usize()
    );
    assert_ne!(LAST_UCONTEXT_PC.load(Ordering::SeqCst), 0);

    println!("signal-test: blocking SIGUSR1");
    let usr1_set = sigset_of(SigNo::SIGUSR1);
    signal::sigprocmask(SigProcMaskHow::Block, Some(&usr1_set), None)?;
    signal::raise(SigNo::SIGUSR1)?;
    assert_eq!(SIMPLE_COUNT.load(Ordering::SeqCst), 1);

    println!("signal-test: unblocking SIGUSR1");
    signal::sigprocmask(SigProcMaskHow::Unblock, Some(&usr1_set), None)?;
    assert_eq!(SIMPLE_COUNT.load(Ordering::SeqCst), 2);

    run_self_signal_stress_test(main_tid)?;
    run_cross_thread_tgkill_test(main_tgid)?;
    run_cross_process_sigqueueinfo_test()?;
    run_realtime_sigqueue_stress_test()?;

    println!("signal-test: all checks passed");
    Ok(())
}
