mod file;
mod timer;

use super::abi::{TimerFdSettimeFlags, TimerFdSpec};

use crate::{
    fs::iomux::PollRoute,
    prelude::*,
    time::{
        monotonic_ns, realtime_read,
        timer::{TimerHandle, cancel_timer_event},
    },
};

use file::TimerFdFile;
pub(super) use file::create_timerfd;
use timer::schedule_timerfd_callback;
#[cfg(feature = "kunit")]
use timer::{timerfd_clock_changed_callback, timerfd_expire_callback};

static_assert!(
    TIMERFD_FILE_MAX_WAITERS > 0,
    "timerfd_file_max_waiters must be nonzero"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TimerFdClock {
    Realtime,
    Monotonic,
    Boottime,
}

#[derive(Clone, Debug)]
struct TimerFdPollRoute {
    route: PollRoute,
    // Diagnostic only: readiness is recomputed under the timerfd state lock.
    interests: PollEvent,
}

impl TimerFdPollRoute {
    fn new(route: &PollRoute, interests: PollEvent) -> Self {
        Self {
            route: route.clone(),
            interests,
        }
    }

    fn is_prunable(&self) -> bool {
        self.route.is_prunable()
    }
}

#[derive(Clone, Debug)]
struct TimerFdIoTrigger {
    trigger: LatchTrigger,
}

impl TimerFdIoTrigger {
    fn new(trigger: &LatchTrigger) -> Self {
        Self {
            trigger: trigger.clone(),
        }
    }

    fn is_prunable(&self) -> bool {
        self.trigger.is_prunable()
    }
}

#[derive(Debug)]
struct TimerFdHandoffBatch {
    // Caller-owned handoff built while holding TimerFdState's no-IRQ lock.
    // Read triggers and stale routes are removed; live poll routes are cloned
    // for notification. Every notify and drop is consumed after unlock.
    read: heapless::Vec<TimerFdIoTrigger, TIMERFD_FILE_MAX_WAITERS>,
    poll_notify: heapless::Vec<TimerFdPollRoute, TIMERFD_FILE_MAX_WAITERS>,
    poll_stale: heapless::Vec<TimerFdPollRoute, TIMERFD_FILE_MAX_WAITERS>,
}

impl TimerFdHandoffBatch {
    fn empty() -> Self {
        Self {
            read: heapless::Vec::new(),
            poll_notify: heapless::Vec::new(),
            poll_stale: heapless::Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.read.is_empty() && self.poll_notify.is_empty() && self.poll_stale.is_empty()
    }

    fn push_read(&mut self, trigger: TimerFdIoTrigger) {
        assert!(
            self.read.push(trigger).is_ok(),
            "timerfd read trigger batch overflow"
        );
    }

    fn push_poll_notify(&mut self, route: TimerFdPollRoute) {
        assert!(
            self.poll_notify.push(route).is_ok(),
            "timerfd poll notify batch overflow"
        );
    }

    fn push_poll_stale(&mut self, route: TimerFdPollRoute) {
        assert!(
            self.poll_stale.push(route).is_ok(),
            "timerfd stale poll route batch overflow"
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Authoritative long-lived timerfd schedule.
///
/// The soft-timer queue owns only the next one-shot request. Periodicity and
/// deadline advancement remain here so replacing or cancelling that request
/// cannot create a second schedule truth.
enum TimerFdSchedule {
    Disarmed,
    Armed {
        deadline: TimerFdDeadline,
        interval_ns: Option<u64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerFdDeadline {
    Monotonic(u64),
    Realtime {
        deadline_ns: u64,
        // Protocol snapshot retained by TimerFdCore for direct read/poll
        // refresh and copied into the one queued soft-timer request.
        cancel_on_change_seq: Option<u64>,
    },
}

impl TimerFdDeadline {
    fn deadline_ns(self) -> u64 {
        match self {
            Self::Monotonic(deadline_ns) | Self::Realtime { deadline_ns, .. } => deadline_ns,
        }
    }

    fn advance(self, delta_ns: u64) -> Self {
        let deadline_ns = self.deadline_ns().saturating_add(delta_ns);
        match self {
            Self::Monotonic(_) => Self::Monotonic(deadline_ns),
            Self::Realtime {
                cancel_on_change_seq,
                ..
            } => Self::Realtime {
                deadline_ns,
                cancel_on_change_seq,
            },
        }
    }
}

#[derive(Debug)]
struct TimerFdState {
    /// Identity of the currently published schedule. Advance it before
    /// cancelling an old request so an already-dequeued callback is harmless.
    generation: u64,
    /// Sole truth for armed/disarmed state and the next logical deadline.
    schedule: TimerFdSchedule,
    /// Cancellation capability for the one queue-owned request corresponding
    /// to `schedule`; dropping the handle alone does not cancel that request.
    request: Option<TimerHandle>,
    /// Unread expirations, saturated because Linux timerfd reports cumulative
    /// expirations and cannot represent more than u64 in one read.
    expirations: u64,
    /// One-shot blocking-read registrations owned while the wait is live.
    read_triggers: Vec<TimerFdIoTrigger>,
    /// Persistent poll subscriptions; prunable entries are detached lazily.
    poll_routes: Vec<TimerFdPollRoute>,
    /// One-shot `ECANCELED` readiness for `TFD_TIMER_CANCEL_ON_SET`. A read
    /// consumes it; a successful settime starts a fresh uncancelled schedule.
    cancelled: bool,
}

impl TimerFdState {
    fn new() -> Result<Self, SysError> {
        Ok(Self {
            generation: 0,
            schedule: TimerFdSchedule::Disarmed,
            request: None,
            expirations: 0,
            read_triggers: queue_with_capacity()?,
            poll_routes: queue_with_capacity()?,
            cancelled: false,
        })
    }

    fn revents(&self, interests: PollEvent) -> PollEvent {
        if interests.contains(PollEvent::READABLE) && (self.cancelled || self.expirations > 0) {
            PollEvent::READABLE
        } else {
            PollEvent::empty()
        }
    }

    fn retire_request(&mut self) {
        // `TimerHandle` is intentionally detachable rather than cancel-on-drop.
        // Invalidate the owner generation first, then attempt physical removal.
        // If the request already reached the threaded lane, its generation
        // check below is the remaining stale-completion barrier.
        self.generation = self
            .generation
            .checked_add(1)
            .expect("timerfd generation space exhausted");
        if let Some(request) = self.request.take() {
            cancel_timer_event(&request);
        }
    }

    fn register_read_wait(
        &mut self,
        trigger: &LatchTrigger,
        stale: &mut TimerFdHandoffBatch,
    ) -> Result<(), SysError> {
        self.detach_prunable_read_triggers(stale);
        if self.read_triggers.len() >= TIMERFD_FILE_MAX_WAITERS {
            return Err(SysError::OutOfMemory);
        }
        self.read_triggers.push(TimerFdIoTrigger::new(trigger));
        Ok(())
    }

    fn register_poll_route(
        &mut self,
        route: &PollRoute,
        interests: PollEvent,
        stale: &mut TimerFdHandoffBatch,
    ) -> bool {
        self.detach_prunable_poll_routes(stale);
        if self.poll_routes.len() >= TIMERFD_FILE_MAX_WAITERS {
            return false;
        }
        self.poll_routes
            .push(TimerFdPollRoute::new(route, interests));
        true
    }

    fn detach_prunable_read_triggers(&mut self, stale: &mut TimerFdHandoffBatch) {
        let mut index = 0;
        while index < self.read_triggers.len() {
            if self.read_triggers[index].is_prunable() {
                stale.push_read(self.read_triggers.swap_remove(index));
            } else {
                index += 1;
            }
        }
    }

    fn detach_prunable_poll_routes(&mut self, stale: &mut TimerFdHandoffBatch) {
        let mut index = 0;
        while index < self.poll_routes.len() {
            if self.poll_routes[index].is_prunable() {
                stale.push_poll_stale(self.poll_routes.swap_remove(index));
            } else {
                index += 1;
            }
        }
    }

    fn collect_readable_waiters(&mut self) -> TimerFdHandoffBatch {
        // Blocking-read triggers remain one-shot and are detached. Poll routes
        // are persistent: clone live routes into the caller-owned notify batch,
        // retain them in the registry, and move only stale routes out. All
        // notification and drop then happens after the no-IRQ guard is released.
        let mut handoff = TimerFdHandoffBatch::empty();
        while let Some(trigger) = self.read_triggers.pop() {
            handoff.push_read(trigger);
        }
        let mut index = 0;
        while index < self.poll_routes.len() {
            if self.poll_routes[index].is_prunable() {
                handoff.push_poll_stale(self.poll_routes.swap_remove(index));
            } else {
                handoff.push_poll_notify(self.poll_routes[index].clone());
                index += 1;
            }
        }
        handoff
    }
}

#[derive(Debug)]
struct TimerFdCore {
    state: NoIrqSpinLock<TimerFdState>,
    clock: TimerFdClock,
}

impl TimerFdCore {
    fn new(clock: TimerFdClock) -> Result<Self, SysError> {
        Ok(Self {
            state: NoIrqSpinLock::new(TimerFdState::new()?),
            clock,
        })
    }
}

impl Drop for TimerFdCore {
    fn drop(&mut self) {
        // Withdraw owner state before touching the queue, and move the handle
        // out so callback destruction/cross-CPU queue locking happens without
        // the timerfd lock. An already-dequeued callback holds only a Weak core
        // and therefore cannot resurrect the object.
        let request = {
            let mut state = self.state.lock();
            state.schedule = TimerFdSchedule::Disarmed;
            state.request.take()
        };
        if let Some(request) = request {
            cancel_timer_event(&request);
        }
    }
}

fn queue_with_capacity<T>() -> Result<Vec<T>, SysError> {
    let mut queue = Vec::new();
    queue
        .try_reserve_exact(TIMERFD_FILE_MAX_WAITERS)
        .map_err(|_| SysError::OutOfMemory)?;
    Ok(queue)
}

fn ns_to_duration(ns: u64) -> Duration {
    Duration::from_nanos(ns)
}

fn deadline_timeout(now_ns: u64, deadline_ns: u64) -> Duration {
    ns_to_duration(deadline_ns.saturating_sub(now_ns))
}

fn deadline_read(deadline: TimerFdDeadline) -> (u64, Option<u64>) {
    match deadline {
        TimerFdDeadline::Monotonic(_) => (monotonic_ns(), None),
        TimerFdDeadline::Realtime { .. } => {
            // Calendar value and change identity must come from one timekeeper
            // snapshot or cancel-on-set could miss a step between two reads.
            let realtime = realtime_read();
            (realtime.now_ns(), Some(realtime.change_seq()))
        },
    }
}

fn snapshot_spec(state: &TimerFdState) -> TimerFdSpec {
    // Remaining time is a projection of the authoritative absolute deadline;
    // it is never cached because realtime steps can change it immediately.
    let interval_ns = match state.schedule {
        TimerFdSchedule::Disarmed => 0,
        TimerFdSchedule::Armed { interval_ns, .. } => interval_ns.unwrap_or(0),
    };
    let value_ns = match state.schedule {
        TimerFdSchedule::Disarmed => 0,
        TimerFdSchedule::Armed { deadline, .. } => {
            let (now_ns, _) = deadline_read(deadline);
            deadline.deadline_ns().saturating_sub(now_ns)
        },
    };
    TimerFdSpec {
        interval_ns: (interval_ns != 0).then_some(interval_ns),
        value_ns,
    }
}

fn replacement_snapshot_spec(state: &TimerFdState) -> TimerFdSpec {
    let TimerFdSchedule::Armed {
        deadline,
        interval_ns,
    } = state.schedule
    else {
        return TimerFdSpec {
            interval_ns: None,
            value_ns: 0,
        };
    };

    let (now_ns, _) = deadline_read(deadline);
    let deadline = replacement_projection_deadline(deadline, interval_ns, now_ns);
    TimerFdSpec {
        interval_ns,
        value_ns: deadline.deadline_ns().saturating_sub(now_ns),
    }
}

fn replacement_projection_deadline(
    deadline: TimerFdDeadline,
    interval_ns: Option<u64>,
    now_ns: u64,
) -> TimerFdDeadline {
    let Some(interval_ns) = interval_ns else {
        return deadline;
    };
    assert!(interval_ns > 0, "armed timerfd interval must be nonzero");
    if now_ns < deadline.deadline_ns() {
        return deadline;
    }

    // settime reports the old periodic timer as if it had advanced from its
    // original target, but replacement must not publish old expirations or
    // queue a successor that will immediately be retired.
    periodic_advance(deadline, interval_ns, now_ns).1
}

fn periodic_advance(
    deadline: TimerFdDeadline,
    interval_ns: u64,
    now_ns: u64,
) -> (u64, TimerFdDeadline) {
    assert!(interval_ns > 0, "armed timerfd interval must be nonzero");
    assert!(
        now_ns >= deadline.deadline_ns(),
        "periodic timerfd advance requires a due deadline"
    );
    let elapsed = now_ns.saturating_sub(deadline.deadline_ns());
    let ticks = (elapsed / interval_ns).saturating_add(1);
    let deadline = deadline.advance(interval_ns.saturating_mul(ticks));
    (ticks, deadline)
}

fn drop_stale_waiters(waiters: TimerFdHandoffBatch, reason: &'static str) {
    if waiters.is_empty() {
        return;
    }
    assert!(
        waiters.poll_notify.is_empty(),
        "timerfd dropped a live poll notification batch"
    );

    kdebugln!(
        "timerfd: dropped stale read={} poll={} waiters reason={}",
        waiters.read.len(),
        waiters.poll_stale.len(),
        reason,
    );
}

fn notify_waiters_after_unlock(waiters: TimerFdHandoffBatch, reason: &'static str) {
    if waiters.is_empty() {
        return;
    }

    kdebugln!(
        "timerfd: detached read={} poll_notify={} poll_stale={} waiters reason={}",
        waiters.read.len(),
        waiters.poll_notify.len(),
        waiters.poll_stale.len(),
        reason,
    );

    for trigger in waiters.read {
        kdebugln!(
            "timerfd: trigger read wait={:#x} reason={}",
            trigger.trigger.wait_id(),
            reason,
        );
        trigger.trigger.trigger();
    }

    for route in waiters.poll_notify {
        kdebugln!(
            "timerfd: notify poll route interests={:?} reason={}",
            route.interests,
            reason,
        );
        route.route.notify();
    }
}

fn refresh_cancel_on_change_snapshot(schedule: &mut TimerFdSchedule) {
    let TimerFdSchedule::Armed {
        deadline:
            TimerFdDeadline::Realtime {
                cancel_on_change_seq,
                ..
            },
        ..
    } = schedule
    else {
        return;
    };
    if cancel_on_change_seq.is_some() {
        // A periodic successor is a new cancel-on-set registration. Steps that
        // preceded this rearm must not cancel the successor retroactively.
        *cancel_on_change_seq = Some(realtime_read().change_seq());
    }
}

fn refresh_due_expiration_locked(
    core: &Arc<TimerFdCore>,
    state: &mut TimerFdState,
) -> TimerFdHandoffBatch {
    let TimerFdSchedule::Armed {
        deadline,
        interval_ns,
    } = state.schedule
    else {
        return TimerFdHandoffBatch::empty();
    };

    let (now_ns, change_seq) = deadline_read(deadline);
    if matches!(
        deadline,
        TimerFdDeadline::Realtime {
            cancel_on_change_seq: Some(armed_seq),
            ..
        } if change_seq.is_some_and(|current_seq| armed_seq < current_seq)
    ) {
        // A direct read/poll may reach the timerfd owner before the threaded
        // clock-change completion. Retire the same request by generation and
        // publish the owner state here so cancellation remains observable once.
        state.retire_request();
        state.schedule = TimerFdSchedule::Disarmed;
        state.expirations = 0;
        state.cancelled = true;
        return state.collect_readable_waiters();
    }
    if now_ns < deadline.deadline_ns() {
        return TimerFdHandoffBatch::empty();
    }

    // Read/poll readiness is derived from the timerfd object's clock state, not
    // solely from whether the threaded callback has already run. If a reader
    // observes an overdue timer before the queued completion gets CPU time,
    // advance the object state here and make that queued completion stale.
    // The queued completion may already be waiting on this state lock. Advance
    // identity first, then remove the request if it is still queue-owned.
    state.retire_request();
    let generation = state.generation;
    let (detached, rearm) = account_due_expiration_locked(state, now_ns, deadline, interval_ns);
    if rearm.is_some() {
        refresh_cancel_on_change_snapshot(&mut state.schedule);
        let TimerFdSchedule::Armed { deadline, .. } = state.schedule else {
            unreachable!("periodic timerfd lost its armed schedule")
        };
        state.request = Some(schedule_timerfd_callback(core, generation, deadline));
    }
    detached
}

fn account_due_expiration_locked(
    state: &mut TimerFdState,
    now_ns: u64,
    deadline: TimerFdDeadline,
    interval_ns: Option<u64>,
) -> (TimerFdHandoffBatch, Option<Duration>) {
    let next_expire_at_ns = deadline.deadline_ns();
    if now_ns < next_expire_at_ns {
        return (
            TimerFdHandoffBatch::empty(),
            Some(deadline_timeout(now_ns, next_expire_at_ns)),
        );
    }

    if let Some(interval_ns) = interval_ns {
        // Advance from the previous target, not callback execution time. This
        // both counts missed periods and prevents worker latency from drifting
        // the periodic schedule.
        let (ticks, deadline) = periodic_advance(deadline, interval_ns, now_ns);
        state.expirations = state.expirations.saturating_add(ticks);
        state.schedule = TimerFdSchedule::Armed {
            deadline,
            interval_ns: Some(interval_ns),
        };
        let timeout = deadline_timeout(now_ns, deadline.deadline_ns());
        (state.collect_readable_waiters(), Some(timeout))
    } else {
        state.expirations = state.expirations.saturating_add(1);
        state.schedule = TimerFdSchedule::Disarmed;
        (state.collect_readable_waiters(), None)
    }
}

pub(super) fn gettime(file: &File) -> Result<TimerFdSpec, SysError> {
    let core = TimerFdFile::core_from_file(file)?;
    let (snapshot, due) = {
        let mut state = core.state.lock();
        let due = refresh_due_expiration_locked(&core, &mut state);
        (snapshot_spec(&state), due)
    };
    notify_waiters_after_unlock(due, "gettime_refresh");
    Ok(snapshot)
}

pub(super) fn settime(
    file: &File,
    flags: TimerFdSettimeFlags,
    new_value: TimerFdSpec,
) -> Result<TimerFdSpec, SysError> {
    let TimerFdSpec {
        value_ns,
        interval_ns,
    } = new_value;
    let core = TimerFdFile::core_from_file(file)?;
    if flags.cancel_on_set && (!flags.abstime || core.clock != TimerFdClock::Realtime) {
        // Linux accepts this combination and simply leaves cancel-on-set
        // disabled. Keep Anemone's current EINVAL behavior visible until that
        // compatibility gap is implemented.
        knoticeln!(
            "timerfd_settime: TFD_TIMER_CANCEL_ON_SET without absolute CLOCK_REALTIME is not implemented as a Linux-compatible no-op; clock={:?}, abstime={}; errno=EINVAL",
            core.clock,
            flags.abstime,
        );
        return Err(SysError::InvalidArgument);
    }
    if flags.cancel_on_set {
        // Linux keeps the cancellation enrollment until a later settime drops
        // the flag, including while disarmed and after expiration or a consumed
        // ECANCELED. Anemone currently watches only this armed generation.
        knoticeln!(
            "timerfd_settime: TFD_TIMER_CANCEL_ON_SET enrollment is incomplete; cancellation is observed only while the current timer generation remains armed"
        );
    }

    let prepared = if value_ns == 0 {
        None
    } else if flags.abstime && core.clock == TimerFdClock::Realtime {
        // Capture the calendar value and sequence together. The timer queue's
        // insert-side recheck closes a concurrent step between this snapshot
        // and publication of the request.
        let realtime = realtime_read();
        Some((
            TimerFdDeadline::Realtime {
                deadline_ns: value_ns,
                cancel_on_change_seq: flags.cancel_on_set.then_some(realtime.change_seq()),
            },
            realtime.now_ns(),
        ))
    } else {
        // Relative requests, including relative CLOCK_REALTIME, are frozen onto
        // the monotonic timeline at settime. BOOTTIME also maps here because
        // Anemone does not yet account for suspend.
        let now_ns = monotonic_ns();
        let deadline_ns = if flags.abstime {
            value_ns
        } else {
            now_ns
                .checked_add(value_ns)
                .ok_or(SysError::InvalidArgument)?
        };
        Some((TimerFdDeadline::Monotonic(deadline_ns), now_ns))
    };
    let mut detached = TimerFdHandoffBatch::empty();

    let old_value = {
        let mut state = core.state.lock();
        let old_value = replacement_snapshot_spec(&state);

        // Retire the previous generation before publishing any replacement.
        // This orders an already-dequeued old callback behind stale identity.
        state.retire_request();
        state.cancelled = false;
        state.expirations = 0;

        if let Some((deadline, now_ns)) = prepared {
            state.schedule = TimerFdSchedule::Armed {
                deadline,
                interval_ns,
            };
            let (new_detached, rearm) =
                account_due_expiration_locked(&mut state, now_ns, deadline, interval_ns);
            detached = new_detached;
            if rearm.is_some() {
                // Normal settime has no recoverable timer-core submit failure:
                // return from the timer submit is the point that lets this armed
                // generation become visible to readers.
                let TimerFdSchedule::Armed { deadline, .. } = state.schedule else {
                    unreachable!("timerfd rearm lost its schedule")
                };
                state.request = Some(schedule_timerfd_callback(&core, state.generation, deadline));
            }
        } else {
            state.schedule = TimerFdSchedule::Disarmed;
        }

        old_value
    };

    notify_waiters_after_unlock(detached, "settime");

    Ok(old_value)
}

#[cfg(feature = "kunit")]
mod kunits {
    use core::mem::size_of;

    use super::*;
    use crate::{
        fs::iomux::IomuxWaitRound,
        time::timer::{queued_timer_count, timer_event_is_queued},
    };

    fn timer_spec(value_sec: u64, interval_sec: u64) -> TimerFdSpec {
        TimerFdSpec {
            value_ns: value_sec.checked_mul(1_000_000_000).unwrap(),
            interval_ns: (interval_sec != 0)
                .then(|| interval_sec.checked_mul(1_000_000_000).unwrap()),
        }
    }

    fn relative_flags() -> TimerFdSettimeFlags {
        TimerFdSettimeFlags {
            abstime: false,
            cancel_on_set: false,
        }
    }

    fn realtime_absolute_flags(cancel_on_set: bool) -> TimerFdSettimeFlags {
        TimerFdSettimeFlags {
            abstime: true,
            cancel_on_set,
        }
    }

    #[kunit]
    fn replace_disarm_and_last_close_remove_queued_requests() {
        let cpu = cur_cpu_id();
        let baseline = queued_timer_count(cpu);
        let file = create_timerfd(TimerFdClock::Monotonic).unwrap();

        settime(&file, relative_flags(), timer_spec(3600, 0)).unwrap();
        {
            let timerfd = TimerFdFile::from_file(&file).unwrap();
            let state = timerfd.core.state.lock();
            assert!(timer_event_is_queued(state.request.as_ref().unwrap()));
        }
        assert_eq!(queued_timer_count(cpu), baseline + 1);

        settime(&file, relative_flags(), timer_spec(1800, 0)).unwrap();
        assert_eq!(queued_timer_count(cpu), baseline + 1);
        settime(&file, relative_flags(), timer_spec(0, 0)).unwrap();
        assert_eq!(queued_timer_count(cpu), baseline);

        settime(&file, relative_flags(), timer_spec(3600, 0)).unwrap();
        assert_eq!(queued_timer_count(cpu), baseline + 1);
        drop(file);
        assert_eq!(queued_timer_count(cpu), baseline);
    }

    #[kunit]
    fn overdue_refresh_physically_removes_the_queued_completion() {
        let cpu = cur_cpu_id();
        let baseline = queued_timer_count(cpu);
        let file = create_timerfd(TimerFdClock::Monotonic).unwrap();
        settime(&file, relative_flags(), timer_spec(3600, 0)).unwrap();

        let detached = {
            let timerfd = TimerFdFile::from_file(&file).unwrap();
            let mut state = timerfd.core.state.lock();
            state.schedule = TimerFdSchedule::Armed {
                deadline: TimerFdDeadline::Monotonic(0),
                interval_ns: None,
            };
            refresh_due_expiration_locked(&timerfd.core, &mut state)
        };
        assert!(detached.is_empty());
        assert_eq!(queued_timer_count(cpu), baseline);
        let timerfd = TimerFdFile::from_file(&file).unwrap();
        let state = timerfd.core.state.lock();
        assert_eq!(state.expirations, 1);
        assert_eq!(state.schedule, TimerFdSchedule::Disarmed);
        assert!(state.request.is_none());
    }

    #[kunit]
    fn periodic_accounting_advances_from_the_previous_target() {
        let mut state = TimerFdState::new().unwrap();
        state.schedule = TimerFdSchedule::Armed {
            deadline: TimerFdDeadline::Monotonic(10),
            interval_ns: Some(10),
        };
        let (_, timeout) =
            account_due_expiration_locked(&mut state, 35, TimerFdDeadline::Monotonic(10), Some(10));
        assert_eq!(state.expirations, 3);
        assert_eq!(
            state.schedule,
            TimerFdSchedule::Armed {
                deadline: TimerFdDeadline::Monotonic(40),
                interval_ns: Some(10),
            }
        );
        assert_eq!(timeout, Some(Duration::from_nanos(5)));
    }

    #[kunit]
    fn gettime_refreshes_overdue_periodic_owner_and_rearms_successor() {
        let cpu = cur_cpu_id();
        let baseline = queued_timer_count(cpu);
        let file = create_timerfd(TimerFdClock::Monotonic).unwrap();
        let periodic = timer_spec(3600, 3600);
        let interval_ns = periodic.interval_ns.unwrap();
        settime(&file, relative_flags(), periodic).unwrap();

        let core = TimerFdFile::core_from_file(&file).unwrap();
        let stale_generation = {
            let mut state = core.state.lock();
            let stale_generation = state.generation;
            assert!(cancel_timer_event(state.request.as_ref().unwrap()));
            state.schedule = TimerFdSchedule::Armed {
                deadline: TimerFdDeadline::Monotonic(0),
                interval_ns: Some(interval_ns),
            };
            stale_generation
        };

        let snapshot = gettime(&file).unwrap();
        assert_eq!(snapshot.interval_ns, Some(interval_ns));
        assert!(snapshot.value_ns > 0);
        assert!(snapshot.value_ns <= interval_ns);
        let state = core.state.lock();
        assert!(state.expirations > 0);
        assert!(matches!(state.schedule, TimerFdSchedule::Armed { .. }));
        assert!(timer_event_is_queued(state.request.as_ref().unwrap()));
        let refreshed_expirations = state.expirations;
        drop(state);
        assert_eq!(queued_timer_count(cpu), baseline + 1);

        // Model an old threaded completion that left the queue before gettime's
        // refresh. The refreshed generation must make it harmless.
        timerfd_expire_callback(Arc::downgrade(&core), stale_generation);
        let state = core.state.lock();
        assert_eq!(state.expirations, refreshed_expirations);
        assert!(timer_event_is_queued(state.request.as_ref().unwrap()));
        drop(state);
        drop(core);
        drop(file);
        assert_eq!(queued_timer_count(cpu), baseline);
    }

    #[kunit]
    fn settime_old_value_projects_periodic_deadline_without_old_rearm() {
        let cpu = cur_cpu_id();
        let baseline = queued_timer_count(cpu);
        let file = create_timerfd(TimerFdClock::Monotonic).unwrap();
        let periodic = timer_spec(3600, 3600);
        let interval_ns = periodic.interval_ns.unwrap();
        settime(&file, relative_flags(), periodic).unwrap();

        let core = TimerFdFile::core_from_file(&file).unwrap();
        {
            let mut state = core.state.lock();
            state.expirations = 7;
            state.schedule = TimerFdSchedule::Armed {
                deadline: TimerFdDeadline::Monotonic(0),
                interval_ns: Some(interval_ns),
            };
        }

        let old = settime(&file, relative_flags(), timer_spec(7200, 0)).unwrap();
        assert_eq!(old.interval_ns, Some(interval_ns));
        assert!(old.value_ns > 0);
        assert!(old.value_ns <= interval_ns);
        let state = core.state.lock();
        assert_eq!(state.expirations, 0);
        assert!(matches!(
            state.schedule,
            TimerFdSchedule::Armed {
                interval_ns: None,
                ..
            }
        ));
        assert!(timer_event_is_queued(state.request.as_ref().unwrap()));
        drop(state);
        assert_eq!(queued_timer_count(cpu), baseline + 1);
        drop(core);
        drop(file);
        assert_eq!(queued_timer_count(cpu), baseline);
    }

    #[kunit]
    fn poll_route_notifies_after_unlock_and_reuses_stale_capacity() {
        let file = create_timerfd(TimerFdClock::Monotonic).unwrap();
        let core = TimerFdFile::core_from_file(&file).unwrap();

        let ready_round = IomuxWaitRound::begin_current();
        let ready_request = ready_round.poll_request(PollEvent::READABLE);
        {
            let mut state = core.state.lock();
            state.expirations = 1;
        }
        assert_eq!(
            file.poll(&ready_request).unwrap(),
            PollRegisterResult::Subscribed(PollEvent::READABLE)
        );
        assert_eq!(core.state.lock().poll_routes.len(), 1);
        ready_round.cancel(LatchCancelReason::PredicateReady);
        let _ = ready_round.finish();

        let notified_round = IomuxWaitRound::begin_current();
        let notified_request = notified_round.poll_request(PollEvent::READABLE);
        {
            let mut state = core.state.lock();
            state.expirations = 0;
        }
        for _ in 0..TIMERFD_FILE_MAX_WAITERS {
            assert_eq!(
                file.poll(&notified_request).unwrap(),
                PollRegisterResult::Subscribed(PollEvent::empty())
            );
        }
        assert_eq!(
            core.state.lock().poll_routes.len(),
            TIMERFD_FILE_MAX_WAITERS
        );
        assert_eq!(
            file.poll(&notified_request).unwrap(),
            PollRegisterResult::Unsupported
        );

        let detached = {
            let mut state = core.state.lock();
            account_due_expiration_locked(&mut state, 1, TimerFdDeadline::Monotonic(1), None).0
        };
        assert_eq!(detached.poll_notify.len(), TIMERFD_FILE_MAX_WAITERS);
        assert!(detached.poll_stale.is_empty());
        assert_eq!(
            core.state.lock().poll_routes.len(),
            TIMERFD_FILE_MAX_WAITERS
        );
        notify_waiters_after_unlock(detached, "kunit_expire");

        notified_round.schedule_with_timeout(Some(Duration::from_secs(1)));
        assert_eq!(notified_round.finish(), LatchWaitOutcome::Triggered);

        let reused_round = IomuxWaitRound::begin_current();
        let reused_request = reused_round.poll_request(PollEvent::READABLE);
        core.state.lock().expirations = 0;
        assert_eq!(
            file.poll(&reused_request).unwrap(),
            PollRegisterResult::Subscribed(PollEvent::empty())
        );
        assert_eq!(core.state.lock().poll_routes.len(), 1);
        reused_round.cancel(LatchCancelReason::PredicateReady);
        let _ = reused_round.finish();
    }

    #[kunit]
    fn cancel_on_set_requires_absolute_realtime_without_mutating_existing_request() {
        let file = create_timerfd(TimerFdClock::Monotonic).unwrap();
        settime(&file, relative_flags(), timer_spec(3600, 0)).unwrap();
        let core = TimerFdFile::core_from_file(&file).unwrap();
        let generation = core.state.lock().generation;

        assert_eq!(
            settime(
                &file,
                TimerFdSettimeFlags {
                    abstime: true,
                    cancel_on_set: true,
                },
                timer_spec(3600, 0),
            ),
            Err(SysError::InvalidArgument)
        );
        let state = core.state.lock();
        assert_eq!(state.generation, generation);
        assert!(matches!(
            state.schedule,
            TimerFdSchedule::Armed {
                deadline: TimerFdDeadline::Monotonic(_),
                ..
            }
        ));
        assert!(timer_event_is_queued(state.request.as_ref().unwrap()));
        drop(state);

        let realtime_file = create_timerfd(TimerFdClock::Realtime).unwrap();
        assert_eq!(
            settime(
                &realtime_file,
                TimerFdSettimeFlags {
                    abstime: false,
                    cancel_on_set: true,
                },
                timer_spec(3600, 0),
            ),
            Err(SysError::InvalidArgument)
        );
    }

    #[kunit]
    fn relative_realtime_timer_is_fixed_to_monotonic_domain() {
        let file = create_timerfd(TimerFdClock::Realtime).unwrap();
        settime(&file, relative_flags(), timer_spec(3600, 0)).unwrap();
        let core = TimerFdFile::core_from_file(&file).unwrap();
        let state = core.state.lock();
        assert!(matches!(
            state.schedule,
            TimerFdSchedule::Armed {
                deadline: TimerFdDeadline::Monotonic(_),
                ..
            }
        ));
        assert!(timer_event_is_queued(state.request.as_ref().unwrap()));
    }

    #[kunit]
    fn absolute_realtime_cancel_is_physical_and_read_reports_ecanceled_once() {
        let cpu = cur_cpu_id();
        let baseline = queued_timer_count(cpu);
        let file = create_timerfd(TimerFdClock::Realtime).unwrap();
        let target_ns = realtime_read()
            .now_ns()
            .checked_add(timer_spec(3600, 0).value_ns)
            .unwrap();
        settime(
            &file,
            realtime_absolute_flags(true),
            TimerFdSpec {
                interval_ns: None,
                value_ns: target_ns,
            },
        )
        .unwrap();
        let core = TimerFdFile::core_from_file(&file).unwrap();
        let generation = core.state.lock().generation;
        {
            let state = core.state.lock();
            assert!(matches!(
                state.schedule,
                TimerFdSchedule::Armed {
                    deadline: TimerFdDeadline::Realtime {
                        cancel_on_change_seq: Some(_),
                        ..
                    },
                    ..
                }
            ));
            assert!(cancel_timer_event(state.request.as_ref().unwrap()));
        }
        assert_eq!(queued_timer_count(cpu), baseline);

        timerfd_clock_changed_callback(Arc::downgrade(&core), generation);
        {
            let state = core.state.lock();
            assert!(state.cancelled);
            assert!(state.request.is_none());
            assert_eq!(state.schedule, TimerFdSchedule::Disarmed);
            assert_eq!(state.revents(PollEvent::READABLE), PollEvent::READABLE);
        }
        let mut value = [0_u8; size_of::<u64>()];
        assert_eq!(file.read(&mut value), Err(SysError::OperationCancelled));
        assert!(!core.state.lock().cancelled);
    }

    #[kunit]
    fn stale_clock_change_completion_cannot_cancel_a_replacement() {
        let file = create_timerfd(TimerFdClock::Realtime).unwrap();
        let target_ns = realtime_read()
            .now_ns()
            .checked_add(timer_spec(3600, 0).value_ns)
            .unwrap();
        let spec = TimerFdSpec {
            interval_ns: None,
            value_ns: target_ns,
        };
        settime(&file, realtime_absolute_flags(true), spec).unwrap();
        let core = TimerFdFile::core_from_file(&file).unwrap();
        let stale_generation = core.state.lock().generation;
        {
            let state = core.state.lock();
            assert!(cancel_timer_event(state.request.as_ref().unwrap()));
        }

        settime(&file, realtime_absolute_flags(false), spec).unwrap();
        timerfd_clock_changed_callback(Arc::downgrade(&core), stale_generation);
        let state = core.state.lock();
        assert!(!state.cancelled);
        assert!(matches!(
            state.schedule,
            TimerFdSchedule::Armed {
                deadline: TimerFdDeadline::Realtime {
                    cancel_on_change_seq: None,
                    ..
                },
                ..
            }
        ));
        assert!(timer_event_is_queued(state.request.as_ref().unwrap()));
    }
}
