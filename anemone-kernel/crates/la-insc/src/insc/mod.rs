//! Privileged instructions wrapper

mod cpucfg;
mod invtlb;
pub use cpucfg::*;
pub use invtlb::*;

/// Read the current time from the stable time source
pub fn rdtime(counter_id: usize) -> u64 {
    let time: u64;
    unsafe {
        core::arch::asm!(
            "rdtime.d {0}, {1}",
            out(reg) time,
             in(reg) counter_id,
        );
    }
    time
}
