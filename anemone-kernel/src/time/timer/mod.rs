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
pub(crate) use threaded::schedule_realtime_threaded_timer_event;
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
    RealtimeThreaded {
        expired: Box<dyn FnOnce() + Send + 'static>,
        clock_changed: Option<Box<dyn FnOnce() + Send + 'static>>,
    },
}

impl Debug for TimerLane {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Irq(_) => f.write_str("Irq"),
            Self::Threaded(_) => f.write_str("Threaded"),
            Self::RealtimeThreaded { .. } => f.write_str("RealtimeThreaded"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerDeadline {
    Monotonic(Instant),
    Realtime {
        deadline_ns: u64,
        cancel_on_change_seq: Option<u64>,
    },
}

impl TimerDeadline {
    fn sort_key(self) -> u64 {
        match self {
            Self::Monotonic(deadline) => deadline.mono(),
            Self::Realtime { deadline_ns, .. } => deadline_ns,
        }
    }

    fn is_realtime(self) -> bool {
        matches!(self, Self::Realtime { .. })
    }
}

struct TimerEvent {
    deadline: TimerDeadline,
    event_id: u64,
    lane: TimerLane,
}

impl TimerEvent {
    fn new(deadline: Instant, event_id: u64, lane: TimerLane) -> Self {
        Self {
            deadline: TimerDeadline::Monotonic(deadline),
            event_id,
            lane,
        }
    }

    fn new_realtime(
        deadline_ns: u64,
        cancel_on_change_seq: Option<u64>,
        event_id: u64,
        lane: TimerLane,
    ) -> Self {
        Self {
            deadline: TimerDeadline::Realtime {
                deadline_ns,
                cancel_on_change_seq,
            },
            event_id,
            lane,
        }
    }

    fn precedes(&self, other: &Self) -> bool {
        assert_eq!(
            self.deadline.is_realtime(),
            other.deadline.is_realtime(),
            "soft timer heap mixed clock domains"
        );
        (self.deadline.sort_key(), self.event_id) < (other.deadline.sort_key(), other.event_id)
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
    realtime_events: Vec<TimerEvent>,
    /// Performance/protocol snapshot of the timekeeper sequence already
    /// scanned by this queue. It never determines calendar time. Insert-side
    /// recheck closes the race where an old-sequence request is added after a
    /// scanner has advanced this snapshot.
    observed_realtime_change_seq: u64,
}

impl TimerQueue {
    const fn new() -> Self {
        Self {
            events: Vec::new(),
            realtime_events: Vec::new(),
            observed_realtime_change_seq: 0,
        }
    }

    fn len(&self) -> usize {
        self.events.len() + self.realtime_events.len()
    }

    fn push(&mut self, event: TimerEvent) {
        assert!(!event.deadline.is_realtime());
        Self::push_into(&mut self.events, event);
    }

    fn push_realtime(&mut self, event: TimerEvent) {
        assert!(event.deadline.is_realtime());
        Self::push_into(&mut self.realtime_events, event);
    }

    fn pop_expired(&mut self, now: Instant) -> Option<TimerEvent> {
        if self.events.first()?.deadline.sort_key() > now.mono() {
            return None;
        }
        Some(Self::remove_from(&mut self.events, 0))
    }

    fn pop_ready_realtime(
        &mut self,
        now_ns: u64,
        change_seq: u64,
    ) -> Option<(TimerEvent, RealtimeReadyCause)> {
        // A scanner may be delayed behind a newer backward step. Once this
        // queue has observed the newer sequence, the older scanner's calendar
        // value is stale and must not expire or cancel any request.
        if change_seq < self.observed_realtime_change_seq {
            return None;
        }
        if self.observed_realtime_change_seq < change_seq {
            if let Some(index) = self.realtime_events.iter().position(|event| {
                matches!(
                    event.deadline,
                    TimerDeadline::Realtime {
                        cancel_on_change_seq: Some(armed_seq),
                        ..
                    } if armed_seq < change_seq
                )
            }) {
                return Some((
                    Self::remove_from(&mut self.realtime_events, index),
                    RealtimeReadyCause::ClockChanged,
                ));
            }
            self.observed_realtime_change_seq = change_seq;
        }

        if self.realtime_events.first()?.deadline.sort_key() > now_ns {
            return None;
        }
        Some((
            Self::remove_from(&mut self.realtime_events, 0),
            RealtimeReadyCause::Expired,
        ))
    }

    fn take_ready_realtime(
        &mut self,
        event_id: u64,
        now_ns: u64,
        change_seq: u64,
    ) -> Option<(TimerEvent, RealtimeReadyCause)> {
        if change_seq < self.observed_realtime_change_seq {
            return None;
        }
        let index = self
            .realtime_events
            .iter()
            .position(|event| event.event_id == event_id)?;
        let event = &self.realtime_events[index];
        let TimerDeadline::Realtime {
            deadline_ns,
            cancel_on_change_seq,
        } = event.deadline
        else {
            unreachable!("realtime heap contained a monotonic deadline")
        };
        let cause = if cancel_on_change_seq.is_some_and(|armed_seq| armed_seq < change_seq) {
            RealtimeReadyCause::ClockChanged
        } else if deadline_ns <= now_ns {
            RealtimeReadyCause::Expired
        } else {
            return None;
        };
        Some((Self::remove_from(&mut self.realtime_events, index), cause))
    }

    fn remove(&mut self, event_id: u64) -> Option<TimerEvent> {
        if let Some(index) = self
            .events
            .iter()
            .position(|event| event.event_id == event_id)
        {
            return Some(Self::remove_from(&mut self.events, index));
        }
        let index = self
            .realtime_events
            .iter()
            .position(|event| event.event_id == event_id)?;
        Some(Self::remove_from(&mut self.realtime_events, index))
    }

    fn push_into(events: &mut Vec<TimerEvent>, event: TimerEvent) {
        events.push(event);
        let index = events.len() - 1;
        Self::sift_up(events, index);
    }

    fn remove_from(events: &mut Vec<TimerEvent>, index: usize) -> TimerEvent {
        let removed = events.swap_remove(index);
        if index == events.len() {
            return removed;
        }

        if index > 0 && events[index].precedes(&events[(index - 1) / 2]) {
            Self::sift_up(events, index);
        } else {
            Self::sift_down(events, index);
        }
        removed
    }

    fn sift_up(events: &mut [TimerEvent], mut index: usize) {
        while index > 0 {
            let parent = (index - 1) / 2;
            if !events[index].precedes(&events[parent]) {
                break;
            }
            events.swap(index, parent);
            index = parent;
        }
    }

    fn sift_down(events: &mut [TimerEvent], mut index: usize) {
        loop {
            let left = index * 2 + 1;
            if left >= events.len() {
                break;
            }
            let right = left + 1;
            let child = if right < events.len() && events[right].precedes(&events[left]) {
                right
            } else {
                left
            };
            if !events[child].precedes(&events[index]) {
                break;
            }
            events.swap(index, child);
            index = child;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RealtimeReadyCause {
    Expired,
    ClockChanged,
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

fn push_realtime_timer_event(
    deadline_ns: u64,
    cancel_on_change_seq: Option<u64>,
    lane: TimerLane,
) -> TimerHandle {
    let event_id =
        try_allocate_event_id(&NEXT_EVENT_ID).expect("soft timer event identity space exhausted");
    let owner_cpu = cur_cpu_id();
    TIMER_QUEUE.with(|queue| {
        queue.lock().push_realtime(TimerEvent::new_realtime(
            deadline_ns,
            cancel_on_change_seq,
            event_id,
            lane,
        ))
    });
    let handle = TimerHandle {
        owner_cpu,
        event_id,
    };

    // Registration happens after the caller's timekeeper snapshot. Recheck the
    // published request so a concurrent step cannot fall between snapshot and
    // queue insertion.
    let realtime = realtime_read();
    if let Some((event, cause)) = TIMER_QUEUE.with(|queue| {
        queue
            .lock()
            .take_ready_realtime(event_id, realtime.now_ns(), realtime.change_seq())
    }) {
        dispatch_ready_event(owner_cpu, event, Some(cause));
    }
    handle
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
        let realtime = realtime_read();
        let events = TIMER_QUEUE.with(|queue| {
            let mut queue = queue.lock();
            // This batch bounds each IRQ critical section. Remaining expired
            // events stay queued and are handled by the next loop iteration.
            let mut events = heapless::Vec::<(TimerEvent, Option<RealtimeReadyCause>), 8>::new();
            let now = Instant::now();
            while !events.is_full() {
                let Some(event) = queue.pop_expired(now) else {
                    break;
                };
                events.push((event, None)).unwrap();
            }
            while !events.is_full() {
                let Some(event) =
                    queue.pop_ready_realtime(realtime.now_ns(), realtime.change_seq())
                else {
                    break;
                };
                events.push((event.0, Some(event.1))).unwrap();
            }

            events
        });
        if events.is_empty() {
            break;
        }
        for (event, cause) in events {
            dispatch_ready_event(cur_cpu_id(), event, cause);
        }
    }
}

/// Recheck every CPU's absolute realtime requests after a committed step.
///
/// The caller holds no timekeeper lock. Each CPU queue is locked independently,
/// and callbacks are handed to threaded workers only after that queue unlocks.
pub(crate) fn recheck_realtime_requests() {
    for cpu in 0..ncpus() {
        let cpu = CpuId::new(cpu);
        loop {
            // Re-read for each CPU so an older concurrent scanner never applies
            // a stale sequence after a newer step has already committed.
            let realtime = realtime_read();
            let events = with_timer_queue(cpu, |queue| {
                let mut queue = queue.lock();
                let mut events = heapless::Vec::<(TimerEvent, RealtimeReadyCause), 8>::new();
                while !events.is_full() {
                    let Some(event) =
                        queue.pop_ready_realtime(realtime.now_ns(), realtime.change_seq())
                    else {
                        break;
                    };
                    events.push(event).unwrap();
                }
                events
            });
            if events.is_empty() {
                break;
            }
            for (event, cause) in events {
                dispatch_ready_event(cpu, event, Some(cause));
            }
        }
    }
}

fn with_timer_queue<R>(cpu: CpuId, f: impl FnOnce(&NoIrqSpinLock<TimerQueue>) -> R) -> R {
    if cpu == cur_cpu_id() {
        TIMER_QUEUE.with(f)
    } else {
        // No caller of this helper holds another CPU queue lock.
        unsafe { TIMER_QUEUE.with_remote(cpu, f) }
    }
}

fn dispatch_ready_event(owner_cpu: CpuId, event: TimerEvent, cause: Option<RealtimeReadyCause>) {
    match event.lane {
        TimerLane::Irq(callback) => {
            assert!(cause.is_none(), "realtime request used the IRQ lane");
            assert_eq!(owner_cpu, cur_cpu_id(), "remote IRQ callback dispatch");
            (callback)();
        },
        TimerLane::Threaded(callback) => {
            assert!(
                cause.is_none(),
                "monotonic callback carried a realtime cause"
            );
            threaded::enqueue_expired_threaded_on(owner_cpu, callback);
        },
        TimerLane::RealtimeThreaded {
            expired,
            clock_changed,
        } => {
            let callback = match cause.expect("realtime callback is missing its completion cause") {
                RealtimeReadyCause::Expired => {
                    drop(clock_changed);
                    expired
                },
                RealtimeReadyCause::ClockChanged => {
                    drop(expired);
                    clock_changed.expect("cancel-on-set request has no clock-change callback")
                },
            };
            threaded::enqueue_expired_threaded_on(owner_cpu, callback);
        },
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
            let queue = queue.lock();
            queue
                .events
                .iter()
                .chain(queue.realtime_events.iter())
                .any(|event| event.event_id == handle.event_id)
        })
    } else {
        unsafe {
            TIMER_QUEUE.with_remote(handle.owner_cpu, |queue| {
                let queue = queue.lock();
                queue
                    .events
                    .iter()
                    .chain(queue.realtime_events.iter())
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

    fn test_realtime_event(
        deadline_ns: u64,
        event_id: u64,
        cancel_on_change_seq: Option<u64>,
    ) -> TimerEvent {
        TimerEvent::new_realtime(
            deadline_ns,
            cancel_on_change_seq,
            event_id,
            TimerLane::RealtimeThreaded {
                expired: Box::new(|| {}),
                clock_changed: cancel_on_change_seq
                    .map(|_| Box::new(|| {}) as Box<dyn FnOnce() + Send + 'static>),
            },
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

    #[kunit]
    fn realtime_heap_expires_forward_and_preserves_requests_after_backward_reads() {
        let mut queue = TimerQueue::new();
        queue.push_realtime(test_realtime_event(100, 1, None));

        assert!(queue.pop_ready_realtime(90, 0).is_none());
        assert!(queue.pop_ready_realtime(20, 0).is_none());
        let (event, cause) = queue.pop_ready_realtime(100, 0).unwrap();
        assert_eq!(event.event_id, 1);
        assert_eq!(cause, RealtimeReadyCause::Expired);
    }

    #[kunit]
    fn realtime_heap_cancels_only_after_its_armed_sequence() {
        let mut queue = TimerQueue::new();
        queue.push_realtime(test_realtime_event(u64::MAX, 1, Some(3)));

        assert!(queue.pop_ready_realtime(0, 3).is_none());
        let (event, cause) = queue.pop_ready_realtime(0, 4).unwrap();
        assert_eq!(event.event_id, 1);
        assert_eq!(cause, RealtimeReadyCause::ClockChanged);
    }

    #[kunit]
    fn stale_realtime_scanner_cannot_cancel_a_newer_request_or_move_sequence_back() {
        let mut queue = TimerQueue::new();
        queue.observed_realtime_change_seq = 5;
        queue.push_realtime(test_realtime_event(u64::MAX, 1, Some(5)));
        queue.push_realtime(test_realtime_event(50, 2, None));

        assert!(queue.pop_ready_realtime(100, 4).is_none());
        assert_eq!(queue.observed_realtime_change_seq, 5);
        assert!(queue.take_ready_realtime(1, 0, 5).is_none());
        let (expired, cause) = queue.pop_ready_realtime(100, 5).unwrap();
        assert_eq!(expired.event_id, 2);
        assert_eq!(cause, RealtimeReadyCause::Expired);
        assert_eq!(
            queue.take_ready_realtime(1, 0, 6).unwrap().1,
            RealtimeReadyCause::ClockChanged
        );
    }

    #[kunit]
    fn insert_side_recheck_closes_snapshot_registration_window() {
        let mut queue = TimerQueue::new();
        queue.observed_realtime_change_seq = 8;
        queue.push_realtime(test_realtime_event(u64::MAX, 1, Some(7)));

        // The scanner already processed sequence 8 before this old-snapshot
        // request was inserted, so only the insert-side named recheck can take it.
        assert_eq!(
            queue.take_ready_realtime(1, 0, 8).unwrap().1,
            RealtimeReadyCause::ClockChanged
        );
        assert!(queue.realtime_events.is_empty());
    }

    #[kunit]
    fn all_cpu_realtime_recheck_dispatches_on_the_request_owner_lane() {
        if ncpus() < 2 {
            return;
        }
        let owner_cpu = (0..ncpus())
            .map(CpuId::new)
            .find(|cpu| *cpu != cur_cpu_id() && target_online(*cpu))
            .expect("SMP KUnit requires an online remote CPU");
        let completed = Arc::new(Event::new());
        let done = Arc::new(AtomicBool::new(false));
        let callback_completed = completed.clone();
        let callback_done = done.clone();
        let event_id = try_allocate_event_id(&NEXT_EVENT_ID).unwrap();
        let now_ns = realtime_read().now_ns();
        let event = TimerEvent::new_realtime(
            now_ns,
            None,
            event_id,
            TimerLane::RealtimeThreaded {
                expired: Box::new(move || {
                    assert_eq!(cur_cpu_id(), owner_cpu);
                    callback_done.store(true, Ordering::Release);
                    callback_completed.publish(usize::MAX, true);
                }),
                clock_changed: None,
            },
        );
        with_timer_queue(owner_cpu, |queue| queue.lock().push_realtime(event));

        recheck_realtime_requests();
        let timeout = completed.listen_with_timeout(
            false,
            || done.load(Ordering::Acquire),
            Duration::from_secs(1),
        );
        assert!(
            !matches!(timeout, Some(TimeoutListenException::Timeout)),
            "remote realtime callback did not complete"
        );
    }
}
