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
    fn current_ticks() -> u64 {
        riscv::register::time::read() as u64
    }

    fn hw_freq_hz() -> Option<u64> {
        unsafe { CLOCK_FREQUENCY_HZ }
    }

    fn set_next_trigger(ticks: u64) {
        sbi_rt::set_timer(Self::current_ticks().wrapping_add(ticks)).expect("Sbi set_timer failed");
    }
}
