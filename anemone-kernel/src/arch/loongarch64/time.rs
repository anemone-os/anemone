//! Reference:
//! - https://elixir.bootlin.com/linux/v7.0-rc6/source/arch/loongarch/include/asm/time.h

use crate::prelude::*;
use crate::sync::mono::MonoOnce;
use la_insc::{
    insc::rdtime,
    reg::{
        csr::{cntc, tcfg, ticlr, tid},
        timer::Tcfg,
    },
};

pub struct LA64TimeArch;

/// Hardware timer frequency in hertz, discovered from firmware.
static CLOCK_FREQUENCY_HZ: MonoOnce<u64> = unsafe { MonoOnce::new() };

/// Shared correction written to each CPU's local CNTC register before that CPU
/// publishes timekeeping readiness.
static CLOCK_COUNTER_OFFSET: MonoOnce<u64> = unsafe { MonoOnce::new() };

/// Record the timer frequency reported by firmware.
pub unsafe fn set_hw_clock_freq(freq_hz: u64) {
    assert!(freq_hz > 0, "LoongArch stable counter frequency must be nonzero");
    CLOCK_FREQUENCY_HZ.init(|slot| {
        slot.write(freq_hz);
    });
}

impl TimeArchTrait for LA64TimeArch {
    type LocalClockSource = Self;
    type LocalClockEvent = Self;
}

impl LocalClockSourceArch for LA64TimeArch {
    fn curr_monotonic_time() -> u64 {
        // RDTIME reads the architecture stable counter shared by all cores;
        // the instruction's second output is only a constant-counter ID.
        rdtime()
    }

    fn monotonic_freq_hz() -> u64 {
        *CLOCK_FREQUENCY_HZ.get()
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
    /// Establish the correction that makes every CPU's RDTIME value part of one
    /// counter domain. Architecturally, `RDTIME = raw counter + CNTC`; subtracting
    /// the inherited CNTC therefore recovers raw counter before choosing the
    /// shared zero point. This is the same `-(drdtime() - CNTC)` correction used
    /// by Linux before writing CNTC on every secondary CPU.
    pub fn init_shared_counter_offset() {
        let inherited_offset = unsafe { cntc::csr_read() };
        let raw_counter = rdtime().wrapping_sub(inherited_offset);
        let shared_offset = 0u64.wrapping_sub(raw_counter);
        CLOCK_COUNTER_OFFSET.init(|slot| {
            slot.write(shared_offset);
        });
        unsafe { cntc::csr_write(shared_offset) };
    }

    /// Initialize the shared source frequency from CPUCFG before AP startup
    /// when firmware did not publish `timebase-frequency`.
    pub fn init_clock_source_from_cpucfg() {
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

        let extensions = rd_cpucfg(2);
        assert!(
            extensions & LOONGARCH_EXT_LLFTP != 0,
            "llftp extension not supported, cannot determine timer frequency"
        );

        let base_freq = rd_cpucfg(4);
        let ratio = rd_cpucfg(5);
        let multiplier = ratio & 0xffff;
        let divisor = (ratio >> 16) & 0xffff;
        assert!(divisor != 0, "LoongArch stable counter divisor is zero");
        let freq_hz = (base_freq as u64 * multiplier as u64) / divisor as u64;

        unsafe { set_hw_clock_freq(freq_hz) };
        knoticeln!("detected timer frequency: {} Hz", freq_hz);
    }

    pub fn claim_timer_interrupt() {
        unsafe {
            ticlr::csr_write(1);
        }
    }

    /// This does not program the first timer interrupt.
    pub fn init_this_cpu() {
        unsafe {
            // CNTC is CPU-local. Every AP must install the BSP-owned offset
            // before the common timekeeper may observe its RDTIME value.
            cntc::csr_write(*CLOCK_COUNTER_OFFSET.get());
            tid::csr_write(cur_cpu_id().physical_id().get() as u32);
        }
    }
}
