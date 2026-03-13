// TODO: a better panic handler with more information

use crate::prelude::*;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    broadcast_ipi(IpiPayload::StopExecution, false);

    // TODO: What if other CPUs are already stuck due to some other cause? Then
    // following code will never be executed, and shutdown will never be called.
    // we should implement async IPIs to handle this case.

    kemergln!("Kernel panic:\n{}", info);
    unsafe { PowerArch::shutdown() }
}
