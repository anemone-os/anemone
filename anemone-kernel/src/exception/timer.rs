//! Timer interrupt handling.

use crate::prelude::*;

/// As Title.
pub fn handle_timer_interrupt() {
    on_timer_interrupt();
    debug_assert!(IntrArch::local_intr_disabled());
    unsafe {
        try_schedule();
    }
}
