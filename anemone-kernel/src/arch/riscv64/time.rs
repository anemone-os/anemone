use crate::prelude::*;
use crate::sync::mono::MonoOnce;

pub struct RiscV64TimeArch;

/// The frequency of the hardware timer in hertz.
static CLOCK_FREQUENCY_HZ: MonoOnce<u64> = unsafe { MonoOnce::new() };

/// Set the frequency of the timer in hertz.
pub unsafe fn set_hw_clock_freq(freq_hz: u64) {
    assert!(freq_hz > 0, "RISC-V timebase frequency must be nonzero");
    CLOCK_FREQUENCY_HZ.init(|slot| {
        slot.write(freq_hz);
    });
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
        *CLOCK_FREQUENCY_HZ.get()
    }
}

impl LocalClockEventArch for RiscV64TimeArch {
    fn program_next_timer(deadline: u64) {
        sbi_rt::set_timer(deadline).expect("Sbi set_timer failed");
    }
}
