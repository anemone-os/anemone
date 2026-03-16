// TODO: a better panic handler with more information

use crate::prelude::*;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kemergln!("Kernel panic:\n{}", info);

    if broadcast_ipi_async(IpiPayload::StopExecution).is_err() {
        kemergln!("failed to broadcast stop execution IPI to other cores during panic");
    }

    unsafe {
        power_off();
    }
}
