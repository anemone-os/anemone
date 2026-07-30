use anemone_abi::time::linux::{TimeSpec, TimeVal};

use crate::{prelude::*, sys::linux::time};

pub fn gettimeofday() -> Result<TimeVal, Errno> {
    let mut tv = TimeVal::default();
    time::gettimeofday(&mut tv as *mut TimeVal as u64, 0).map(|_| tv)
}

pub fn nanosleep(duration: TimeSpec) -> Result<(), Errno> {
    time::nanosleep(&duration as *const TimeSpec as u64, 0).map(|_| ())
}
