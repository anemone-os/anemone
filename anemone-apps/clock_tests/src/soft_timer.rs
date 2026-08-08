//! Userspace regression for cancellable soft-timer consumers.
//!
//! Far-future replacement loops exercise physical request retirement; short
//! periodic and signal cases then prove live completion still reaches timerfd,
//! ITIMER_REAL, and an interruptible sleep.

use core::{
    mem::size_of,
    sync::atomic::{AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        process::linux::signal::{SigAction, SigSet},
        time::linux::{
            ITimerSpec, TimeSpec, TimeVal,
            clock::CLOCK_MONOTONIC,
            itimer::{ITIMER_REAL, OldITimerVal},
        },
    },
    os::linux::{
        fs::{close, read},
        process::signal::{SigNo, sigaction},
        time,
    },
    prelude::*,
};

static SIGALRM_DELIVERIES: AtomicUsize = AtomicUsize::new(0);

extern "C" fn sigalrm_handler(_signo: i32) {
    SIGALRM_DELIVERIES.fetch_add(1, Ordering::Relaxed);
}

fn timerfd_settime(fd: u32, value: ITimerSpec) {
    time::timerfd_settime(fd, 0, &value, None).unwrap();
}

fn timerfd_gettime(fd: u32) -> ITimerSpec {
    time::timerfd_gettime(fd).unwrap()
}

fn set_real_itimer(value: OldITimerVal) {
    time::setitimer(ITIMER_REAL, &value, None).unwrap();
}

fn get_real_itimer() -> OldITimerVal {
    time::getitimer(ITIMER_REAL).unwrap()
}

fn verify_timerfd_replace_periodic_and_close() {
    let fd = time::timerfd_create(CLOCK_MONOTONIC, 0).unwrap();

    // Every replacement names a far-future request. Implementations that keep
    // stale requests queued accumulate all 64 instead of retaining one live arm.
    for seconds in 3600..3664 {
        timerfd_settime(
            fd,
            ITimerSpec {
                it_interval: TimeSpec::default(),
                it_value: TimeSpec {
                    tv_sec: seconds,
                    tv_nsec: 0,
                },
            },
        );
    }
    assert!(timerfd_gettime(fd).it_value.tv_sec > 0);
    timerfd_settime(fd, ITimerSpec::default());
    assert_eq!(timerfd_gettime(fd), ITimerSpec::default());

    timerfd_settime(
        fd,
        ITimerSpec {
            it_interval: TimeSpec {
                tv_sec: 0,
                tv_nsec: 10_000_000,
            },
            it_value: TimeSpec {
                tv_sec: 0,
                tv_nsec: 20_000_000,
            },
        },
    );
    let mut expirations = [0u8; size_of::<u64>()];
    assert_eq!(read(fd, &mut expirations).unwrap(), expirations.len());
    assert!(u64::from_le_bytes(expirations) >= 1);
    close(fd).unwrap();
}

fn verify_itimer_signal_interrupts_nanosleep() {
    // Mirror the timerfd replacement pressure through the thread-group owner.
    for seconds in 3600..3664 {
        set_real_itimer(OldITimerVal {
            it_interval: TimeVal::default(),
            it_value: TimeVal {
                tv_sec: seconds,
                tv_usec: 0,
            },
        });
    }
    assert!(get_real_itimer().it_value.tv_sec > 0);
    set_real_itimer(OldITimerVal::default());
    assert_eq!(get_real_itimer(), OldITimerVal::default());

    // The short one-shot proves the surviving request commits SIGALRM outside
    // the itimer lock and interrupts the ordinary nanosleep wait round.
    SIGALRM_DELIVERIES.store(0, Ordering::Relaxed);
    let action = SigAction {
        sighandler: (sigalrm_handler as *const ()).into(),
        sa_flags: 0,
        sa_restorer: anemone_rs::abi::RawUserAddr64::NULL,
        sa_mask: SigSet { bits: 0 },
    };
    let mut old_action = SigAction {
        sighandler: anemone_rs::abi::RawUserAddr64::NULL,
        sa_flags: 0,
        sa_restorer: anemone_rs::abi::RawUserAddr64::NULL,
        sa_mask: SigSet { bits: 0 },
    };
    sigaction(SigNo::SIGALRM, Some(&action), Some(&mut old_action)).unwrap();
    set_real_itimer(OldITimerVal {
        it_interval: TimeVal::default(),
        it_value: TimeVal {
            tv_sec: 0,
            tv_usec: 20_000,
        },
    });

    let sleep = TimeSpec {
        tv_sec: 1,
        tv_nsec: 0,
    };
    let result = time::nanosleep(sleep);
    assert_eq!(result, Err(EINTR));
    assert_eq!(SIGALRM_DELIVERIES.load(Ordering::Relaxed), 1);
    assert_eq!(get_real_itimer(), OldITimerVal::default());
    sigaction(SigNo::SIGALRM, Some(&old_action), None).unwrap();
}

pub(crate) fn verify_soft_timer_consumers() {
    verify_timerfd_replace_periodic_and_close();
    verify_itimer_signal_interrupts_nanosleep();
    println!("soft-timer: timerfd, ITIMER_REAL, and interrupted nanosleep checks passed");
}
