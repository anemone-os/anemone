use crate::prelude::*;

pub struct LA64TimeArch;

/// The frequency of the hardware timer in hertz.
static mut CLOCK_FREQUENCY_HZ: Option<u64> = None;

/// Set the frequency of the timer in hertz.
pub unsafe fn set_hw_clock_freq(freq_hz: u64) {
    unsafe {
        CLOCK_FREQUENCY_HZ = Some(freq_hz);
    }
}

impl TimeArchTrait for LA64TimeArch {
    fn current_ticks() -> u64 {
        todo!()
    }

    fn hw_freq_hz() -> Option<u64> {
        unsafe { CLOCK_FREQUENCY_HZ }
    }

    fn set_next_trigger(ticks: u64) {
        todo!()
    }
}
