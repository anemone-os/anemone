//! Privileged instructions wrapper

mod cpucfg;
mod invtlb;
pub use cpucfg::*;
pub use invtlb::*;

/// Read the current time from the stable time source
pub fn rdtime() -> u64 {
    let time: u64;
    unsafe {
        core::arch::asm!(
            "rdtime.d {time}, {counter_id}",
            time = out(reg) time,
            // The second architectural output is the constant-counter ID. It
            // is diagnostic identity, not an input to or owner of the time value.
            counter_id = out(reg) _,
        );
    }
    time
}
