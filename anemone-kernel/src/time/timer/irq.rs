use crate::prelude::*;

use super::{TimerEvent, expire_ticks_after, push_timer_event};

/// Schedule a timer event to run in interrupt context on the local CPU after
/// the given duration.
///
/// # Safety
///
/// The callback runs in timer IRQ context. It must not sleep, block, take
/// ordinary mutexes, or perform work that relies on process context.
pub unsafe fn schedule_local_irq_timer_event(
    expire: Duration,
    callback: Box<dyn FnOnce() + Send + 'static>,
) {
    push_timer_event(TimerEvent::new_irq(expire_ticks_after(expire), callback));
}
