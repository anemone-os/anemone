//! Soft timer.
//!
//! Timer events are one-shot completions. Callers explicitly choose whether a
//! completion runs in timer IRQ context or on the bounded threaded completion
//! lane. The timer core owns only the queued request and its cancellation
//! identity; object generation, wait identity, and periodic semantics remain
//! with the submitting owner.

mod irq;
mod threaded;

use core::fmt::Debug;

use crate::prelude::*;

pub use irq::schedule_local_irq_timer_event;
pub use threaded::schedule_threaded_timer_event;

static NEXT_EVENT_ID: AtomicU64 = AtomicU64::new(1);

/// Opaque capability naming one request while it remains in a per-CPU queue.
#[must_use = "retain the handle for cancellation or explicitly discard a fire-and-forget request"]
#[derive(Debug)]
pub struct TimerHandle {
    owner_cpu: CpuId,
    event_id: u64,
}

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
    event_id: u64,
    lane: TimerLane,
}

impl TimerEvent {
    fn new(deadline: Instant, event_id: u64, lane: TimerLane) -> Self {
        Self {
            deadline,
            event_id,
            lane,
        }
    }

    fn precedes(&self, other: &Self) -> bool {
        (self.deadline, self.event_id) < (other.deadline, other.event_id)
    }
}

impl Debug for TimerEvent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TimerEvent")
            .field("deadline", &self.deadline)
            .field("event_id", &self.event_id)
            .field("lane", &self.lane)
            .finish()
    }
}

#[derive(Debug)]
struct TimerQueue {
    events: Vec<TimerEvent>,
}

impl TimerQueue {
    const fn new() -> Self {
        Self { events: Vec::new() }
    }

    fn len(&self) -> usize {
        self.events.len()
    }

    fn push(&mut self, event: TimerEvent) {
        self.events.push(event);
        self.sift_up(self.events.len() - 1);
    }

    fn pop_expired(&mut self, now: Instant) -> Option<TimerEvent> {
        if self.events.first()?.deadline > now {
            return None;
        }
        Some(self.remove_at(0))
    }

    fn remove(&mut self, event_id: u64) -> Option<TimerEvent> {
        let index = self
            .events
            .iter()
            .position(|event| event.event_id == event_id)?;
        Some(self.remove_at(index))
    }

    fn remove_at(&mut self, index: usize) -> TimerEvent {
        let removed = self.events.swap_remove(index);
        if index == self.events.len() {
            return removed;
        }

        if index > 0 && self.events[index].precedes(&self.events[(index - 1) / 2]) {
            self.sift_up(index);
        } else {
            self.sift_down(index);
        }
        removed
    }

    fn sift_up(&mut self, mut index: usize) {
        while index > 0 {
            let parent = (index - 1) / 2;
            if !self.events[index].precedes(&self.events[parent]) {
                break;
            }
            self.events.swap(index, parent);
            index = parent;
        }
    }

    fn sift_down(&mut self, mut index: usize) {
        loop {
            let left = index * 2 + 1;
            if left >= self.events.len() {
                break;
            }
            let right = left + 1;
            let child =
                if right < self.events.len() && self.events[right].precedes(&self.events[left]) {
                    right
                } else {
                    left
                };
            if !self.events[child].precedes(&self.events[index]) {
                break;
            }
            self.events.swap(index, child);
            index = child;
        }
    }
}

#[percpu]
static TIMER_QUEUE: NoIrqSpinLock<TimerQueue> = NoIrqSpinLock::new(TimerQueue::new());

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

fn try_allocate_event_id(allocator: &AtomicU64) -> Option<u64> {
    allocator
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |event_id| {
            event_id.checked_add(1)
        })
        .ok()
}

fn push_timer_event(deadline: Instant, lane: TimerLane) -> TimerHandle {
    let event_id =
        try_allocate_event_id(&NEXT_EVENT_ID).expect("soft timer event identity space exhausted");
    let owner_cpu = cur_cpu_id();
    TIMER_QUEUE.with(|queue| queue.lock().push(TimerEvent::new(deadline, event_id, lane)));
    TimerHandle {
        owner_cpu,
        event_id,
    }
}

/// Remove a request if it is still owned by its per-CPU timer queue.
///
/// `false` means the request was already removed or dequeued to an execution
/// lane. In the latter case, the submitting owner must reject stale work by its
/// generation, validness, or wait identity.
pub fn cancel_timer_event(handle: &TimerHandle) -> bool {
    let event = if handle.owner_cpu == cur_cpu_id() {
        TIMER_QUEUE.with(|queue| queue.lock().remove(handle.event_id))
    } else {
        // The target queue has an IRQ-safe lock and this operation holds no
        // other CPU queue lock. The removed callback is dropped after unlock.
        unsafe {
            TIMER_QUEUE.with_remote(handle.owner_cpu, |queue| {
                queue.lock().remove(handle.event_id)
            })
        }
    };
    event.is_some()
}

pub fn on_timer_interrupt() {
    assert!(IntrArch::local_intr_disabled());

    loop {
        let events = TIMER_QUEUE.with(|queue| {
            let mut queue = queue.lock();
            // This batch bounds each IRQ critical section. Remaining expired
            // events stay queued and are handled by the next loop iteration.
            let mut events = heapless::Vec::<TimerEvent, 8>::new();
            let now = Instant::now();
            while !events.is_full() {
                let Some(event) = queue.pop_expired(now) else {
                    break;
                };
                events.push(event).unwrap();
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

#[cfg(feature = "kunit")]
pub(crate) fn queued_timer_count(cpu: CpuId) -> usize {
    if cpu == cur_cpu_id() {
        TIMER_QUEUE.with(|queue| queue.lock().len())
    } else {
        unsafe { TIMER_QUEUE.with_remote(cpu, |queue| queue.lock().len()) }
    }
}

#[cfg(feature = "kunit")]
pub(crate) fn timer_event_is_queued(handle: &TimerHandle) -> bool {
    if handle.owner_cpu == cur_cpu_id() {
        TIMER_QUEUE.with(|queue| {
            queue
                .lock()
                .events
                .iter()
                .any(|event| event.event_id == handle.event_id)
        })
    } else {
        unsafe {
            TIMER_QUEUE.with_remote(handle.owner_cpu, |queue| {
                queue
                    .lock()
                    .events
                    .iter()
                    .any(|event| event.event_id == handle.event_id)
            })
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{
        sched::oneshot::{Sender, channel},
        task::kthread::{KThreadBuilder, KThreadCtx},
        utils::any_opaque::AnyOpaque,
    };

    fn test_event(deadline: u64, event_id: u64) -> TimerEvent {
        TimerEvent::new(
            Instant::from_mono(deadline),
            event_id,
            TimerLane::Irq(Box::new(|| {})),
        )
    }

    fn assert_heap_order(queue: &TimerQueue) {
        for index in 1..queue.events.len() {
            let parent = (index - 1) / 2;
            assert!(!queue.events[index].precedes(&queue.events[parent]));
        }
    }

    #[kunit]
    fn same_deadline_requests_keep_independent_identity_order() {
        let mut queue = TimerQueue::new();
        queue.push(test_event(20, 4));
        queue.push(test_event(10, 3));
        queue.push(test_event(10, 1));
        queue.push(test_event(10, 2));
        assert_heap_order(&queue);

        let now = Instant::from_mono(10);
        assert_eq!(queue.pop_expired(now).unwrap().event_id, 1);
        assert_eq!(queue.pop_expired(now).unwrap().event_id, 2);
        assert_eq!(queue.pop_expired(now).unwrap().event_id, 3);
        assert!(queue.pop_expired(now).is_none());
        assert_eq!(
            queue.pop_expired(Instant::from_mono(20)).unwrap().event_id,
            4
        );
    }

    #[kunit]
    fn removing_arbitrary_requests_repairs_the_single_heap() {
        let mut queue = TimerQueue::new();
        for (deadline, event_id) in [(8, 8), (2, 2), (6, 6), (1, 1), (4, 4), (3, 3)] {
            queue.push(test_event(deadline, event_id));
        }
        assert_eq!(queue.remove(4).unwrap().event_id, 4);
        assert!(queue.remove(4).is_none());
        assert_heap_order(&queue);

        for expected in [1, 2, 3, 6, 8] {
            assert_eq!(
                queue
                    .pop_expired(Instant::from_mono(u64::MAX))
                    .unwrap()
                    .event_id,
                expected
            );
            assert_heap_order(&queue);
        }
    }

    #[kunit]
    fn event_identity_exhaustion_fails_without_reuse() {
        let allocator = AtomicU64::new(u64::MAX - 1);
        assert_eq!(try_allocate_event_id(&allocator), Some(u64::MAX - 1));
        assert_eq!(try_allocate_event_id(&allocator), None);
        assert_eq!(allocator.load(Ordering::Relaxed), u64::MAX);
        assert_eq!(try_allocate_event_id(&allocator), None);
    }

    struct CallbackDropProbe;

    impl Drop for CallbackDropProbe {
        fn drop(&mut self) {
            let unlocked = TIMER_QUEUE.with(|queue| queue.try_lock().is_some());
            assert!(unlocked, "timer callback was dropped under the queue lock");
        }
    }

    #[kunit]
    fn cancel_drops_callback_after_unlock_and_is_idempotent() {
        let baseline = queued_timer_count(cur_cpu_id());
        let probe = CallbackDropProbe;
        let request = unsafe {
            schedule_local_irq_timer_event(Duration::from_secs(3600), Box::new(move || drop(probe)))
        };
        assert_eq!(queued_timer_count(cur_cpu_id()), baseline + 1);
        assert!(timer_event_is_queued(&request));
        assert!(cancel_timer_event(&request));
        assert!(!cancel_timer_event(&request));
        assert_eq!(queued_timer_count(cur_cpu_id()), baseline);
    }

    #[kunit]
    fn cancel_after_irq_dequeue_reports_already_dequeued() {
        let fired = Arc::new(AtomicBool::new(false));
        let callback_fired = fired.clone();
        let request = unsafe {
            schedule_local_irq_timer_event(
                Duration::ZERO,
                Box::new(move || callback_fired.store(true, Ordering::Release)),
            )
        };
        with_intr_disabled(on_timer_interrupt);
        assert!(fired.load(Ordering::Acquire));
        assert!(!cancel_timer_event(&request));
    }

    #[derive(Opaque)]
    struct RemoteSchedule {
        owner_cpu: CpuId,
        sender: Option<Sender<TimerHandle>>,
    }

    fn schedule_remote_request(_: KThreadCtx, mut opaque: AnyOpaque) -> i32 {
        let context = opaque
            .cast_mut::<RemoteSchedule>()
            .expect("invalid remote timer KUnit context");
        assert_eq!(cur_cpu_id(), context.owner_cpu);
        let request =
            unsafe { schedule_local_irq_timer_event(Duration::from_secs(3600), Box::new(|| {})) };
        context
            .sender
            .take()
            .expect("remote timer sender was consumed")
            .send(request)
            .expect("remote timer receiver was dropped");
        0
    }

    #[kunit]
    fn remote_cpu_cancel_removes_only_the_named_request() {
        if ncpus() < 2 {
            return;
        }
        let owner_cpu = (0..ncpus())
            .map(CpuId::new)
            .find(|cpu| *cpu != cur_cpu_id() && target_online(*cpu))
            .expect("SMP KUnit requires an online remote CPU");
        let baseline = queued_timer_count(owner_cpu);
        let (sender, receiver) = channel();
        let worker = KThreadBuilder::new("kunit:timer-remote-schedule")
            .cpu(owner_cpu)
            .spawn(
                schedule_remote_request,
                AnyOpaque::new(RemoteSchedule {
                    owner_cpu,
                    sender: Some(sender),
                }),
            )
            .expect("failed to spawn remote timer scheduler");
        let request = receiver
            .recv_uninterruptible()
            .expect("remote timer scheduler closed early");
        assert!(timer_event_is_queued(&request));
        assert_eq!(queued_timer_count(owner_cpu), baseline + 1);
        assert!(cancel_timer_event(&request));
        assert!(!cancel_timer_event(&request));
        assert_eq!(queued_timer_count(owner_cpu), baseline);
        assert_eq!(worker.wait_exited(), 0);
    }

    #[kunit]
    fn repeated_far_future_schedule_cancel_returns_to_baseline() {
        let cpu = cur_cpu_id();
        let baseline = queued_timer_count(cpu);
        for _ in 0..128 {
            let request = unsafe {
                schedule_local_irq_timer_event(Duration::from_secs(3600), Box::new(|| {}))
            };
            assert!(cancel_timer_event(&request));
        }
        assert_eq!(queued_timer_count(cpu), baseline);
    }
}
