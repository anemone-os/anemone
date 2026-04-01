use crate::prelude::*;
use la_insc::{
    insc::rdtime,
    reg::{
        csr::{tcfg, ticlr, tid},
        timer::Tcfg,
    },
};

pub struct LA64TimeArch;

/// Hardware timer frequency in hertz, discovered from firmware.
static mut CLOCK_FREQUENCY_HZ: Option<u64> = None;

/// Record the timer frequency reported by firmware.
pub unsafe fn set_hw_clock_freq(freq_hz: u64) {
    unsafe {
        CLOCK_FREQUENCY_HZ = Some(freq_hz);
    }
}

impl TimeArchTrait for LA64TimeArch {
    /// Read the current hardware timer tick counter.
    fn current_ticks() -> u64 {
        rdtime(CpuArch::cur_cpu_id().get())
    }

    /// Return the timer frequency reported by firmware, if known.
    fn hw_freq_hz() -> Option<u64> {
        unsafe { CLOCK_FREQUENCY_HZ }
    }

    /// Program the next timer interrupt deadline.
    fn set_next_trigger(ticks: u64) {
        unsafe {
            tcfg::csr_write(Tcfg::new(ticks, true, true));
        }
    }
}

impl LA64TimeArch {
    /// Claim a timer interrupt on the current CPU.
    pub fn claim_timer_interrupt() {
        unsafe {
            ticlr::csr_write(1);
        }
    }

    /// Initialize the current CPU timer state and arm the first tick.
    pub fn init() {
        unsafe {
            tid::csr_write(CpuArch::cur_cpu_id().get() as u32);
            TimeArch::set_next_trigger(300_000_0);
        }
    }
}
