// TODO: a better panic handler with more information

use crate::{debug::backtrace::CapturedBacktrace, prelude::*};

/// TODO: double panic.
static PANIC_OCCURRED: AtomicBool = AtomicBool::new(false);

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    PANIC_OCCURRED.store(true, Ordering::SeqCst);

    unsafe {
        IntrArch::local_intr_disable();
    }
    if let Err(e) = broadcast_ipi_async(IpiPayload::StopExecution) {
        kemergln!(
            "failed to broadcast stop execution IPI to other cores during panic: {:?}",
            e
        );
    }
    kemergln!("Kernel panic at {} :\n{}", cur_cpu_id(), info);
    let backtrace = CapturedBacktrace::capture();
    kemergln!("Backtrace:\n{}", backtrace);

    unsafe {
        power_off();
    }
}
