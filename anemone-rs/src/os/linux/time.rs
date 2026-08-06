use anemone_abi::time::linux::{ITimerSpec, SigEvent, TimeSpec, TimeVal};

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

pub fn clock_settime(clock_id: i32, value: &TimeSpec) -> Result<(), Errno> {
    time::clock_settime(clock_id, value as *const TimeSpec as u64).map(|_| ())
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
