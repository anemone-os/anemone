//! Timer interrupt handling.

use crate::prelude::*;

/// As Title.
pub fn handle_timer_interrupt() {
    on_timer_interrupt();
    unsafe {
        try_schedule();
    }
}
