use crate::prelude::*;

pub struct RiscV64Power;

pub use RiscV64Power as Power;

impl PowerArch for RiscV64Power {
    unsafe fn shutdown() -> ! {
        sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);
        unreachable!()
    }

    unsafe fn reboot() -> ! {
        sbi_rt::system_reset(sbi_rt::ColdReboot, sbi_rt::NoReason);
        unreachable!()
    }
}
