use crate::prelude::*;

pub struct RiscV64TimeArch;

/// The frequency of the hardware timer in hertz.
static mut CLOCK_FREQUENCY_HZ: Option<u64> = None;

/// Set the frequency of the timer in hertz.
pub unsafe fn set_hw_clock_freq(freq_hz: u64) {
    unsafe {
        CLOCK_FREQUENCY_HZ = Some(freq_hz);
    }
}

impl TimeArchTrait for RiscV64TimeArch {
    type LocalClockSource = Self;
    type LocalClockEvent = Self;
}

impl LocalClockSourceArch for RiscV64TimeArch {
    fn curr_monotonic_time() -> u64 {
        riscv::register::time::read64()
    }

    fn monotonic_freq_hz() -> u64 {
        unsafe { CLOCK_FREQUENCY_HZ.expect("clock frequency not set") }
    }
}

impl LocalClockEventArch for RiscV64TimeArch {
    fn program_next_timer(deadline: u64) {
        sbi_rt::set_timer(deadline).expect("Sbi set_timer failed");
    }
}
