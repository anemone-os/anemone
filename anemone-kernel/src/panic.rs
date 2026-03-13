// TODO: a better panic handler with more information

use crate::prelude::*;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    broadcast_ipi_async(IpiPayload::StopExecution).expect("failed to send stop execution IPI");

    kemergln!("Kernel panic:\n{}", info);
    unsafe { PowerArch::shutdown() }
}
