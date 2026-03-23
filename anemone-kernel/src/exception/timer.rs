//! Timer interrupt handling.

use crate::sched::schedule;



/// Handle a timer interrupt from kernel.
pub fn handle_kernel_timer_interrupt() {
    unsafe{
        schedule();
    }
}
