// TODO: a better panic handler with more information
// TODO: if an ap panics, it should notify the bsp.

use crate::prelude::*;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kemergln!("Kernel panic:\n{}", info);
    unsafe { CurPowerArch::shutdown() }
}
