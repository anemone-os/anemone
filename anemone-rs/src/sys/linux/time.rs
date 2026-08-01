use super::*;

pub fn gettimeofday(tv_ptr: u64, tz_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_GETTIMEOFDAY, tv_ptr, tz_ptr, 0, 0, 0, 0) }
}

pub fn nanosleep(duration_ptr: u64, rem_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_NANOSLEEP, duration_ptr, rem_ptr, 0, 0, 0, 0) }
}
