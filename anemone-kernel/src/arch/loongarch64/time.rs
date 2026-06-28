//! Reference:
//! - https://elixir.bootlin.com/linux/v7.0-rc6/source/arch/loongarch/include/asm/time.h

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
    type LocalClockSource = Self;
    type LocalClockEvent = Self;
}

impl LocalClockSourceArch for LA64TimeArch {
    fn curr_monotonic_time() -> u64 {
        rdtime(cur_cpu_id().get())
    }

    fn monotonic_freq_hz() -> u64 {
        unsafe { CLOCK_FREQUENCY_HZ.expect("clock frequency not set") }
    }
}

impl LocalClockEventArch for LA64TimeArch {
    fn program_next_timer(deadline: u64) {
        let countdown = deadline.saturating_sub(Self::curr_monotonic_time()) >> 2;

        unsafe {
            tcfg::csr_write(Tcfg::new(countdown, false, true));
        }
    }
}

impl LA64TimeArch {
    pub fn claim_timer_interrupt() {
        unsafe {
            ticlr::csr_write(1);
        }
    }

    /// This does not program the first timer interrupt.
    pub fn init_this_cpu() {
        unsafe {
            tid::csr_write(cur_cpu_id().get() as u32);
        }

        unsafe {
            if CLOCK_FREQUENCY_HZ == None {
                // device tree does not specify stable time source frequency.
                // we should calculate it from csr.

                // see reference code in linux kernel for more details.

                fn rd_cpucfg(reg: usize) -> u32 {
                    let val: u32;
                    unsafe {
                        core::arch::asm!(
                            "cpucfg {val}, {reg}",
                            val = out(reg) val,
                            reg = in(reg) reg,
                        );
                    }
                    val
                }

                const LOONGARCH_EXT_LLFTP: u32 = bit!(14);

                let extensions: u32 = rd_cpucfg(2);
                if extensions & LOONGARCH_EXT_LLFTP == 0 {
                    panic!("llftp extension not supported, cannot determine timer frequency");
                }

                let base_freq: u32 = rd_cpucfg(4);
                let (cfm, cfd) = {
                    let val = rd_cpucfg(5);
                    let cfm = val & 0xffff;
                    let cfd = (val >> 16) & 0xffff;
                    (cfm, cfd)
                };

                let freq_hz = (base_freq as u64 * cfm as u64) / cfd as u64;

                CLOCK_FREQUENCY_HZ = Some(freq_hz);

                knoticeln!("detected timer frequency: {} Hz", freq_hz);
            }
        }
    }
}
