//! Timer events and related functionality.

use core::fmt::Debug;

use alloc::collections::binary_heap::BinaryHeap;

use crate::prelude::*;

struct TimerEvent {
    expire_ticks: u64,
    callback: Box<dyn FnOnce() + Send + 'static>,
}

impl Debug for TimerEvent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TimerEvent")
            .field("expire_ticks", &self.expire_ticks)
            .finish()
    }
}

impl PartialEq for TimerEvent {
    fn eq(&self, other: &Self) -> bool {
        self.expire_ticks == other.expire_ticks
    }
}

impl Eq for TimerEvent {}

impl PartialOrd for TimerEvent {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        // we do a reverse order here, cz we'll use a max-heap to implement the timer
        // event queue, and we want the event with the smallest expire_ticks to be
        // popped first.
        Some(self.expire_ticks.cmp(&other.expire_ticks).reverse())
    }
}

impl Ord for TimerEvent {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

static IRQ_EVENT_QUEUE: SpinLock<BinaryHeap<TimerEvent>> = SpinLock::new(BinaryHeap::new());
// TODO: threaded_event_queue

/// Schedule a timer event to run in interrupt context after the given duration.
///
/// The callback will be run in interrupt context, so it should not do anything
/// that may sleep or block.
pub unsafe fn schedule_irq_timer_event(
    expire: Duration,
    callback: Box<dyn FnOnce() + Send + 'static>,
) {
    let expire_ticks = ticks() + duration_to_ticks(expire);
    let event = TimerEvent {
        expire_ticks,
        callback,
    };
    IRQ_EVENT_QUEUE.lock_irqsave().push(event);
}

// not a high-priority task. do this later.
pub fn schedule_threaded_timer_event(
    expire: Duration,
    callback: Box<dyn FnOnce() + Send + 'static>,
) {
    todo!()
}

pub fn on_timer_interrupt() {
    // irq event handling
    {
        loop {
            let events = {
                let mut queue = IRQ_EVENT_QUEUE.lock_irqsave();

                // use a statically allocated vector to avoid dynamic memory allocation in the
                // interrupt handler. 8 is actually randomly chosen, which does not make much
                // sense.
                let mut events = heapless::Vec::<TimerEvent, 8>::new();
                while let Some(event) = queue.peek() {
                    if events.is_full() {
                        break;
                    }
                    if event.expire_ticks <= ticks() {
                        events.push(queue.pop().unwrap()).unwrap();
                    } else {
                        break;
                    }
                }

                events
            };
            if events.is_empty() {
                break;
            }
            for event in events {
                (event.callback)();
            }
        }
    }
    // TODO: threaded event handling.
}
