use crate::prelude::*;

pub struct RiscV64PowerArch;

impl PowerArchTrait for RiscV64PowerArch {
    unsafe fn shutdown() -> ! {
        sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);
        unreachable!()
    }

    unsafe fn reboot() -> ! {
        sbi_rt::system_reset(sbi_rt::ColdReboot, sbi_rt::NoReason);
        unreachable!()
    }
}
