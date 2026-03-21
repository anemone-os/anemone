// TODO: a better panic handler with more information

use crate::prelude::*;

pub static PANIC_OCCURRED: AtomicBool = AtomicBool::new(false);

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    PANIC_OCCURRED.store(true, Ordering::SeqCst);

    unsafe {
        IntrArch::local_intr_disable();
    }
    kemergln!("Kernel panic:\n{}", info);
    if let Err(e) = broadcast_ipi_async(IpiPayload::StopExecution) {
        kemergln!(
            "failed to broadcast stop execution IPI to other cores during panic: {:?}",
            e
        );
    }
    unsafe {
        power_off();
    }
}
