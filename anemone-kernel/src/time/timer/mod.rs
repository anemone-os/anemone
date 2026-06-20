//! Soft timer.
//!
//! Timer events are one-shot completions. Callers explicitly choose whether a
//! completion runs in timer IRQ context or on the bounded threaded completion
//! lane. The timer core does not provide cancellation, per-object identity, or
//! periodic semantics.

mod irq;
mod threaded;

use core::fmt::Debug;

use crate::prelude::*;

pub use irq::schedule_local_irq_timer_event;
pub use threaded::schedule_threaded_timer_event;

enum TimerLane {
    Irq(Box<dyn FnOnce() + Send + 'static>),
    Threaded(Box<dyn FnOnce() + Send + 'static>),
}

impl Debug for TimerLane {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Irq(_) => f.write_str("Irq"),
            Self::Threaded(_) => f.write_str("Threaded"),
        }
    }
}

struct TimerEvent {
    expire_ticks: u64,
    lane: TimerLane,
}

impl TimerEvent {
    fn new_irq(expire_ticks: u64, callback: Box<dyn FnOnce() + Send + 'static>) -> Self {
        Self {
            expire_ticks,
            lane: TimerLane::Irq(callback),
        }
    }

    fn new_threaded(expire_ticks: u64, callback: Box<dyn FnOnce() + Send + 'static>) -> Self {
        Self {
            expire_ticks,
            lane: TimerLane::Threaded(callback),
        }
    }
}

impl Debug for TimerEvent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TimerEvent")
            .field("expire_ticks", &self.expire_ticks)
            .field("lane", &self.lane)
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
        // Reverse ordering lets BinaryHeap pop the earliest deadline first.
        Some(self.expire_ticks.cmp(&other.expire_ticks).reverse())
    }
}

impl Ord for TimerEvent {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

#[percpu]
static TIMER_QUEUE: alloc::collections::binary_heap::BinaryHeap<TimerEvent> =
    alloc::collections::binary_heap::BinaryHeap::new();

fn expire_ticks_after(expire: Duration) -> u64 {
    ticks() + duration_to_ticks(expire)
}

fn push_timer_event(event: TimerEvent) {
    with_intr_disabled(|| TIMER_QUEUE.with_mut(|queue| queue.push(event)));
}

pub fn on_timer_interrupt() {
    debug_assert!(IntrArch::local_intr_disabled());

    loop {
        let events = TIMER_QUEUE.with_mut(|queue| {
            // This batch bounds each IRQ critical section. Remaining expired
            // events stay queued and are handled by the next loop iteration.
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
        });
        if events.is_empty() {
            break;
        }
        for event in events {
            match event.lane {
                TimerLane::Irq(callback) => (callback)(),
                TimerLane::Threaded(callback) => threaded::enqueue_expired_threaded(callback),
            }
        }
    }
}
