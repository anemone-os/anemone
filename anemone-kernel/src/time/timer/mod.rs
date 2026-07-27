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
    deadline: Instant,
    lane: TimerLane,
}

impl TimerEvent {
    fn new_irq(deadline: Instant, callback: Box<dyn FnOnce() + Send + 'static>) -> Self {
        Self {
            deadline,
            lane: TimerLane::Irq(callback),
        }
    }

    fn new_threaded(deadline: Instant, callback: Box<dyn FnOnce() + Send + 'static>) -> Self {
        Self {
            deadline,
            lane: TimerLane::Threaded(callback),
        }
    }
}

impl Debug for TimerEvent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TimerEvent")
            .field("deadline", &self.deadline)
            .field("lane", &self.lane)
            .finish()
    }
}

impl PartialEq for TimerEvent {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline
    }
}

impl Eq for TimerEvent {}

impl PartialOrd for TimerEvent {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        // Reverse ordering lets BinaryHeap pop the earliest deadline first.
        Some(self.deadline.cmp(&other.deadline).reverse())
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

fn deadline_after(expire: Duration) -> Instant {
    // A relative tick count loses the current tick phase: at 100 Hz, a 10 ms
    // timeout submitted just before the next tick could expire almost
    // immediately. `Instant::checked_add` converts the duration by rounding
    // down to counter units, so advance every nonzero timeout by one unit to
    // keep the representable deadline on or after the requested instant. An
    // exactly representable timeout may therefore be late by one counter unit,
    // which is below the periodic interrupt's delivery granularity.
    let deadline = Instant::now()
        .checked_add(expire)
        .unwrap_or(Instant::from_mono(u64::MAX));
    if expire.is_zero() {
        deadline
    } else {
        Instant::from_mono(deadline.mono().saturating_add(1))
    }
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
            let now = Instant::now();
            while let Some(event) = queue.peek() {
                if events.is_full() {
                    break;
                }
                if event.deadline <= now {
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
