use la_insc::reg::{csr::{tcfg, ticlr, tid}, timer::Tcfg};
use crate::prelude::*;

pub struct LA64TimeArch;

static mut CLOCK_FREQUENCY_HZ: Option<u64> = None;

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
        unsafe
        {
            tcfg::csr_write(Tcfg::new(ticks, true, true));
        }
    }
}

impl LA64TimeArch{
    /// Claim a timer interrupt on the current CPU.
    pub fn claim_timer_interrupt() {
        unsafe {
            ticlr::csr_write(1);
        }
    }

    /// Initialize and start the timer
    pub fn init(){
        unsafe{
            tid::csr_write(CpuArch::cur_cpu_id() as u32);
             TimeArch::set_next_trigger(300_000_0);
        }
    }
}
