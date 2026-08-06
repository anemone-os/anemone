//! Gate 4 userspace oracle for `SI_TIMER` signal-frame serialization.
//!
//! Linux permits userspace to submit negative `si_code` values through
//! `rt_sigqueueinfo()`. Gate 4 uses that ABI to exercise both architecture
//! frame builders before timer syscalls exist. Gate 5 must replace this
//! injection path as the primary oracle with a real POSIX timer expiry.

use core::sync::atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering};

use anemone_rs::{
    abi::process::linux::{
        signal::{self as linux_signal, SA_SIGINFO, SigAction, SigInfo, SigInfoWrapper, SigSet},
        ucontext::UContext,
    },
    os::linux::process::{
        getpid,
        signal::{self, SigNo, SigProcMaskHow},
    },
    prelude::*,
};

const TIMER_ID: i32 = 0x1234_5678;
const TIMER_OVERRUN: i32 = 37;
const TIMER_SIGVAL: u64 = 0x1357_2468_9abc_def0;

static TIMER_DELIVERIES: AtomicUsize = AtomicUsize::new(0);
static TIMER_CODE: AtomicI32 = AtomicI32::new(0);
static TIMER_ID_SEEN: AtomicI32 = AtomicI32::new(0);
static TIMER_OVERRUN_SEEN: AtomicI32 = AtomicI32::new(0);
static TIMER_SIGVAL_SEEN: AtomicU64 = AtomicU64::new(0);
static TIMER_PRIVATE_SEEN: AtomicI32 = AtomicI32::new(-1);
static ORDINARY_DELIVERIES: AtomicUsize = AtomicUsize::new(0);

#[anemone_rs::signal_handler(siginfo)]
fn timer_handler(signo: SigNo, siginfo: *const SigInfo, ucontext: *const UContext) {
    assert_eq!(signo, SigNo::SIGUSR2);
    assert!(!siginfo.is_null());
    assert!(!ucontext.is_null());

    let info = unsafe { &*siginfo };
    let timer = unsafe { info.fields.timer };
    TIMER_CODE.store(info.si_code, Ordering::SeqCst);
    TIMER_ID_SEEN.store(timer.tid, Ordering::SeqCst);
    TIMER_OVERRUN_SEEN.store(timer.overrun, Ordering::SeqCst);
    TIMER_SIGVAL_SEEN.store(timer.sigval.as_u64(), Ordering::SeqCst);
    TIMER_PRIVATE_SEEN.store(timer.sys_private, Ordering::SeqCst);
    TIMER_DELIVERIES.fetch_add(1, Ordering::SeqCst);
}

#[anemone_rs::signal_handler]
fn ordinary_handler(signo: SigNo) {
    assert_eq!(signo, SigNo::SIGUSR1);
    ORDINARY_DELIVERIES.fetch_add(1, Ordering::SeqCst);
}

fn empty_action() -> SigAction {
    SigAction {
        sighandler: core::ptr::null(),
        sa_flags: 0,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    }
}

fn verify_timer_frame() {
    TIMER_DELIVERIES.store(0, Ordering::SeqCst);
    TIMER_PRIVATE_SEEN.store(-1, Ordering::SeqCst);

    let action = SigAction {
        sighandler: timer_handler as *const (),
        sa_flags: SA_SIGINFO,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    };
    let mut old_action = empty_action();
    signal::sigaction(SigNo::SIGUSR2, Some(&action), Some(&mut old_action)).unwrap();

    let info = SigInfoWrapper {
        info: SigInfo {
            si_signo: SigNo::SIGUSR2.as_usize() as i32,
            si_errno: 0,
            si_code: linux_signal::SI_TIMER,
            fields: linux_signal::sifields::SigInfoFields {
                timer: linux_signal::sifields::Timer {
                    tid: TIMER_ID,
                    overrun: TIMER_OVERRUN,
                    sigval: linux_signal::sifields::SigVal {
                        sival_ptr: TIMER_SIGVAL as *mut _,
                    },
                    // This word is kernel-private. The copy boundary must not
                    // reflect a userspace-supplied value into the frame.
                    sys_private: 0x55,
                },
            },
        },
    };
    signal::sigqueueinfo(getpid().unwrap(), SigNo::SIGUSR2, &info).unwrap();

    assert_eq!(TIMER_DELIVERIES.load(Ordering::SeqCst), 1);
    assert_eq!(TIMER_CODE.load(Ordering::SeqCst), linux_signal::SI_TIMER);
    assert_eq!(TIMER_ID_SEEN.load(Ordering::SeqCst), TIMER_ID);
    assert_eq!(TIMER_OVERRUN_SEEN.load(Ordering::SeqCst), TIMER_OVERRUN);
    assert_eq!(TIMER_SIGVAL_SEEN.load(Ordering::SeqCst), TIMER_SIGVAL);
    assert_eq!(TIMER_PRIVATE_SEEN.load(Ordering::SeqCst), 0);

    signal::sigaction(SigNo::SIGUSR2, Some(&old_action), None).unwrap();
}

fn verify_ordinary_standard_merge() {
    ORDINARY_DELIVERIES.store(0, Ordering::SeqCst);
    let action = SigAction {
        sighandler: ordinary_handler as *const (),
        sa_flags: 0,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    };
    let mut old_action = empty_action();
    signal::sigaction(SigNo::SIGUSR1, Some(&action), Some(&mut old_action)).unwrap();

    let blocked = SigSet {
        // Linux signal sets map signal N to bit N-1.
        bits: 1u64 << (SigNo::SIGUSR1.as_usize() - 1),
    };
    let mut old_mask = SigSet { bits: 0 };
    signal::sigprocmask(SigProcMaskHow::Block, Some(&blocked), Some(&mut old_mask)).unwrap();
    assert_eq!(old_mask.bits & blocked.bits, 0);

    signal::raise(SigNo::SIGUSR1).unwrap();
    signal::raise(SigNo::SIGUSR1).unwrap();
    assert_eq!(ORDINARY_DELIVERIES.load(Ordering::SeqCst), 0);

    signal::sigprocmask(SigProcMaskHow::SetMask, Some(&old_mask), None).unwrap();
    assert_eq!(ORDINARY_DELIVERIES.load(Ordering::SeqCst), 1);
    signal::sigaction(SigNo::SIGUSR1, Some(&old_action), None).unwrap();
}

pub(crate) fn verify_timer_signal_frame() {
    verify_timer_frame();
    verify_ordinary_standard_merge();
    println!("timer-signal: SI_TIMER frame and ordinary standard merge checks passed");
}
