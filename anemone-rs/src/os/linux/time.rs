use anemone_abi::time::linux::{ITimerSpec, SigEvent, TimeSpec, TimeVal, Timex, itimer::OldITimerVal};

use crate::{prelude::*, sys::linux::time};

pub fn gettimeofday() -> Result<TimeVal, Errno> {
    let mut tv = TimeVal::default();
    time::gettimeofday(&mut tv as *mut TimeVal as u64, 0).map(|_| tv)
}

pub fn nanosleep(duration: TimeSpec) -> Result<(), Errno> {
    time::nanosleep(&duration as *const TimeSpec as u64, 0).map(|_| ())
}

pub fn clock_gettime(clock_id: i32) -> Result<TimeSpec, Errno> {
    let mut value = TimeSpec::default();
    time::clock_gettime(clock_id, &mut value as *mut TimeSpec as u64).map(|_| value)
}

pub fn clock_getres(clock_id: i32) -> Result<TimeSpec, Errno> {
    let mut value = TimeSpec::default();
    time::clock_getres(clock_id, &mut value as *mut TimeSpec as u64).map(|_| value)
}

pub fn clock_settime(clock_id: i32, value: &TimeSpec) -> Result<(), Errno> {
    time::clock_settime(clock_id, value as *const TimeSpec as u64).map(|_| ())
}

pub fn clock_nanosleep(
    clock_id: i32,
    flags: i32,
    request: &TimeSpec,
    remaining: Option<&mut TimeSpec>,
) -> Result<(), Errno> {
    time::clock_nanosleep(
        clock_id,
        flags,
        request as *const TimeSpec as u64,
        remaining.map_or(0, |value| value as *mut TimeSpec as u64),
    )
    .map(|_| ())
}

pub fn clock_adjtime(clock_id: i32, value: &mut Timex) -> Result<i32, Errno> {
    time::clock_adjtime(clock_id, value as *mut Timex as u64).map(|result| result as i32)
}

pub fn timerfd_create(clock_id: i32, flags: i32) -> Result<u32, Errno> {
    time::timerfd_create(clock_id, flags).map(|fd| fd as u32)
}

pub fn timerfd_settime(
    fd: u32,
    flags: u32,
    value: &ITimerSpec,
    old_value: Option<&mut ITimerSpec>,
) -> Result<(), Errno> {
    time::timerfd_settime(
        fd,
        flags,
        value as *const ITimerSpec as u64,
        old_value.map_or(0, |value| value as *mut ITimerSpec as u64),
    )
    .map(|_| ())
}

pub fn timerfd_gettime(fd: u32) -> Result<ITimerSpec, Errno> {
    let mut value = ITimerSpec::default();
    time::timerfd_gettime(fd, &mut value as *mut ITimerSpec as u64).map(|_| value)
}

pub fn setitimer(
    which: i32,
    value: &OldITimerVal,
    old_value: Option<&mut OldITimerVal>,
) -> Result<(), Errno> {
    time::setitimer(
        which,
        value as *const OldITimerVal as u64,
        old_value.map_or(0, |value| value as *mut OldITimerVal as u64),
    )
    .map(|_| ())
}

pub fn getitimer(which: i32) -> Result<OldITimerVal, Errno> {
    let mut value = OldITimerVal::default();
    time::getitimer(which, &mut value as *mut OldITimerVal as u64).map(|_| value)
}

pub fn timer_create(clock_id: i32, event: Option<&SigEvent>) -> Result<i32, Errno> {
    // `SigEvent` is the Linux UAPI wire layout; the timer core decodes it at
    // the syscall boundary and does not retain the raw union.
    let mut timer_id = -1_i32;
    time::timer_create(
        clock_id,
        event.map_or(0, |event| event as *const SigEvent as u64),
        &mut timer_id as *mut i32 as u64,
    )
    .map(|_| timer_id)
}

pub fn timer_settime(
    timer_id: i32,
    flags: i32,
    value: &ITimerSpec,
    old_value: Option<&mut ITimerSpec>,
) -> Result<(), Errno> {
    time::timer_settime(
        timer_id,
        flags,
        value as *const ITimerSpec as u64,
        old_value.map_or(0, |value| value as *mut ITimerSpec as u64),
    )
    .map(|_| ())
}

pub fn timer_gettime(timer_id: i32) -> Result<ITimerSpec, Errno> {
    let mut value = ITimerSpec::default();
    time::timer_gettime(timer_id, &mut value as *mut ITimerSpec as u64).map(|_| value)
}

pub fn timer_getoverrun(timer_id: i32) -> Result<i32, Errno> {
    time::timer_getoverrun(timer_id).map(|value| value as i32)
}

pub fn timer_delete(timer_id: i32) -> Result<(), Errno> {
    time::timer_delete(timer_id).map(|_| ())
}
