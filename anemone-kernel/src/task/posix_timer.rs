//! Thread-group-owned POSIX timer objects and ID namespace.
//!
//! Linux UAPI conversion stays in `time::posix_timer::api`. This module owns
//! timer IDs, schedules, generations, periodic accounting, and teardown; the
//! soft-timer and signal subsystems receive only one-shot capabilities.

use crate::{
    prelude::*,
    task::sig::{
        PosixTimerSignalCallback, PosixTimerSignalEnqueue, PosixTimerSignalIdentity,
        PosixTimerSignalRegistration, SigNo,
    },
    time::{
        monotonic_ns, realtime_ns,
        timer::{
            TimerHandle, cancel_timer_event, schedule_realtime_threaded_timer_event,
            schedule_threaded_timer_event,
        },
    },
};

const NSEC_PER_SEC: u64 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PosixTimerClock {
    Realtime,
    Monotonic,
    Boottime,
}

impl PosixTimerClock {
    fn absolute_timeline(self) -> PosixTimerTimeline {
        match self {
            Self::Realtime => PosixTimerTimeline::Realtime,
            Self::Monotonic | Self::Boottime => PosixTimerTimeline::Monotonic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PosixTimerTimeline {
    Realtime,
    Monotonic,
}

impl PosixTimerTimeline {
    fn now_ns(self) -> u64 {
        match self {
            Self::Realtime => realtime_ns(),
            Self::Monotonic => monotonic_ns(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PosixTimerNotification {
    None,
    DefaultSignal,
    Signal { no: SigNo, sigval: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct PosixTimerSetting {
    pub(crate) value_ns: u64,
    pub(crate) interval_ns: u64,
}

#[derive(Debug)]
pub struct PosixTimers {
    table: NoIrqSpinLock<PosixTimerTable>,
}

#[derive(Debug, Default)]
struct PosixTimerTable {
    slots: Vec<Option<PosixTimerSlot>>,
    next_reservation: u64,
}

#[derive(Debug)]
enum PosixTimerSlot {
    /// A create operation owns this numeric ID, but syscall lookup must still
    /// report EINVAL until user copyout succeeds and publication commits. The
    /// token prevents an old create transaction from publishing over an ID
    /// reused after exec teardown clears its reservation.
    Reserved(u64),
    Published(Arc<PosixTimer>),
}

impl PosixTimerTable {
    fn reserve(&mut self) -> Result<(i32, u64), SysError> {
        self.next_reservation = self
            .next_reservation
            .checked_add(1)
            .expect("POSIX timer reservation identity exhausted");
        let token = self.next_reservation;
        if let Some((index, slot)) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.is_none())
        {
            *slot = Some(PosixTimerSlot::Reserved(token));
            return Ok((i32::try_from(index).map_err(|_| SysError::Again)?, token));
        }

        let id = i32::try_from(self.slots.len()).map_err(|_| SysError::Again)?;
        self.slots
            .try_reserve(1)
            .map_err(|_| SysError::OutOfMemory)?;
        self.slots.push(Some(PosixTimerSlot::Reserved(token)));
        Ok((id, token))
    }

    fn release_reservation(&mut self, id: i32, token: u64) {
        let Some(slot) = self.slots.get_mut(id as usize) else {
            return;
        };
        if matches!(slot, Some(PosixTimerSlot::Reserved(current)) if *current == token) {
            *slot = None;
        }
    }

    fn publish(&mut self, id: i32, token: u64, timer: Arc<PosixTimer>) -> Result<(), SysError> {
        let Some(slot) = self.slots.get_mut(id as usize) else {
            return Err(SysError::NoSuchProcess);
        };
        if !matches!(slot, Some(PosixTimerSlot::Reserved(current)) if *current == token) {
            return Err(SysError::NoSuchProcess);
        }
        *slot = Some(PosixTimerSlot::Published(timer));
        Ok(())
    }

    fn get(&self, id: i32) -> Option<Arc<PosixTimer>> {
        if id < 0 {
            return None;
        }
        match self.slots.get(id as usize)?.as_ref()? {
            PosixTimerSlot::Reserved(_) => None,
            PosixTimerSlot::Published(timer) => Some(timer.clone()),
        }
    }

    fn remove(&mut self, id: i32) -> Option<Arc<PosixTimer>> {
        if id < 0 {
            return None;
        }
        match self.slots.get_mut(id as usize)?.take()? {
            PosixTimerSlot::Reserved(_) => None,
            PosixTimerSlot::Published(timer) => Some(timer),
        }
    }

    fn take_all(&mut self) -> Vec<Option<PosixTimerSlot>> {
        core::mem::take(&mut self.slots)
    }
}

impl PosixTimers {
    pub const fn new() -> Self {
        Self {
            table: NoIrqSpinLock::new(PosixTimerTable {
                slots: Vec::new(),
                next_reservation: 0,
            }),
        }
    }
}

impl Drop for PosixTimers {
    fn drop(&mut self) {
        let slots = self.table.lock().take_all();
        delete_slots(slots);
    }
}

/// Prepared create transaction. Drop rolls back both the unpublished ID and
/// any signal resource installed while preparing the object.
pub(crate) struct PreparedPosixTimer {
    owner: Weak<ThreadGroup>,
    id: i32,
    reservation: u64,
    timer: Option<Arc<PosixTimer>>,
}

impl PreparedPosixTimer {
    pub(crate) fn id(&self) -> i32 {
        self.id
    }

    pub(crate) fn publish(mut self) -> Result<(), SysError> {
        let owner = self.owner.upgrade().ok_or(SysError::NoSuchProcess)?;
        let timer = self
            .timer
            .as_ref()
            .expect("prepared POSIX timer was consumed")
            .clone();
        owner
            .posix_timers
            .table
            .lock()
            .publish(self.id, self.reservation, timer)?;
        self.timer.take();
        Ok(())
    }
}

impl Drop for PreparedPosixTimer {
    fn drop(&mut self) {
        if self.timer.is_none() {
            return;
        }
        if let Some(owner) = self.owner.upgrade() {
            owner
                .posix_timers
                .table
                .lock()
                .release_reservation(self.id, self.reservation);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotificationKind {
    None,
    Signal,
}

#[derive(Debug)]
struct PosixTimer {
    id: i32,
    clock: PosixTimerClock,
    notification_kind: NotificationKind,
    /// Installed once before ID publication and removed only after deletion
    /// invalidates the object. It is lifecycle capability, not timer state.
    signal_registration: NoIrqSpinLock<Option<PosixTimerSignalRegistration>>,
    inner: NoIrqSpinLock<PosixTimerInner>,
}

#[derive(Debug)]
struct PosixTimerInner {
    generation: u64,
    deleted: bool,
    arm: Option<PosixTimerArm>,
    next_episode: u64,
    pending: Option<PendingEpisode>,
    last_overrun: i32,
}

#[derive(Debug)]
struct PosixTimerArm {
    /// Relative arms always use monotonic. Only an absolute CLOCK_REALTIME arm
    /// uses the mutable calendar timeline, so date changes cannot alter an
    /// elapsed-duration request.
    timeline: PosixTimerTimeline,
    /// Authoritative target on `timeline`; callback execution time never
    /// replaces this value, so periodic timers cannot accumulate scheduler
    /// drift.
    target_ns: u64,
    interval_ns: u64,
    request: Option<TimerHandle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingEpisode {
    generation: u64,
    episode: u64,
    overrun: u64,
}

impl PosixTimer {
    fn new(id: i32, clock: PosixTimerClock, kind: NotificationKind) -> Self {
        Self {
            id,
            clock,
            notification_kind: kind,
            signal_registration: NoIrqSpinLock::new(None),
            inner: NoIrqSpinLock::new(PosixTimerInner {
                generation: 1,
                deleted: false,
                arm: None,
                next_episode: 0,
                pending: None,
                last_overrun: 0,
            }),
        }
    }

    fn schedule(
        self: &Arc<Self>,
        generation: u64,
        timeline: PosixTimerTimeline,
        target_ns: u64,
    ) -> TimerHandle {
        let timer = Arc::downgrade(self);
        match timeline {
            PosixTimerTimeline::Realtime => schedule_realtime_threaded_timer_event(
                target_ns,
                None,
                Box::new(move || {
                    if let Some(timer) = timer.upgrade() {
                        timer.expire(generation);
                    }
                }),
                None,
            ),
            PosixTimerTimeline::Monotonic => {
                let delay = ns_to_duration(target_ns.saturating_sub(timeline.now_ns()));
                schedule_threaded_timer_event(
                    delay,
                    Box::new(move || {
                        if let Some(timer) = timer.upgrade() {
                            timer.expire(generation);
                        }
                    }),
                )
            },
        }
    }

    fn setting_snapshot(&self) -> PosixTimerSetting {
        let inner = self.inner.lock();
        setting_snapshot(&inner)
    }

    fn settime(
        self: &Arc<Self>,
        setting: PosixTimerSetting,
        absolute: bool,
    ) -> Result<PosixTimerSetting, SysError> {
        let timeline = if absolute {
            self.clock.absolute_timeline()
        } else {
            PosixTimerTimeline::Monotonic
        };
        let now_ns = timeline.now_ns();
        let new_target = if setting.value_ns == 0 {
            None
        } else if absolute {
            Some(setting.value_ns)
        } else {
            Some(
                now_ns
                    .checked_add(setting.value_ns)
                    .ok_or(SysError::InvalidArgument)?,
            )
        };

        let old_request = {
            let mut inner = self.inner.lock();
            if inner.deleted {
                return Err(SysError::InvalidArgument);
            }
            let old = setting_snapshot(&inner);
            inner.generation = inner
                .generation
                .checked_add(1)
                .expect("POSIX timer generation exhausted");
            inner.pending = None;
            let old_request = inner.arm.as_mut().and_then(|arm| arm.request.take());
            inner.arm = new_target.map(|target_ns| {
                let request = self.schedule(inner.generation, timeline, target_ns);
                PosixTimerArm {
                    timeline,
                    target_ns,
                    interval_ns: setting.interval_ns,
                    request: Some(request),
                }
            });
            (old, old_request)
        };
        if let Some(request) = old_request.1 {
            cancel_timer_event(&request);
        }
        Ok(old_request.0)
    }

    fn getoverrun(&self) -> Result<i32, SysError> {
        let inner = self.inner.lock();
        if inner.deleted {
            Err(SysError::InvalidArgument)
        } else {
            Ok(inner.last_overrun)
        }
    }

    fn expire(self: &Arc<Self>, generation: u64) {
        let notification = {
            let mut inner = self.inner.lock();
            if inner.deleted || inner.generation != generation {
                return;
            }
            let Some(mut arm) = inner.arm.take() else {
                return;
            };
            // This callback owns the dequeued request; keeping its stale handle
            // would let later cleanup pretend it can still remove queue state.
            arm.request = None;

            let now_ns = arm.timeline.now_ns();
            if now_ns < arm.target_ns {
                arm.request = Some(self.schedule(generation, arm.timeline, arm.target_ns));
                inner.arm = Some(arm);
                return;
            }

            let expirations = if arm.interval_ns == 0 {
                1
            } else {
                periods_through(arm.target_ns, arm.interval_ns, now_ns)
            };
            if arm.interval_ns != 0 {
                let Some(next_target) = advance_target(arm.target_ns, arm.interval_ns, expirations)
                else {
                    // No representable future target remains. Disarm rather
                    // than creating a wrapped second schedule truth.
                    inner.arm = None;
                    return;
                };
                arm.target_ns = next_target;
                arm.request = Some(self.schedule(generation, arm.timeline, next_target));
                inner.arm = Some(arm);
            }

            match self.notification_kind {
                NotificationKind::None => return,
                NotificationKind::Signal => {
                    let pending = if let Some(pending) = inner.pending.as_mut() {
                        assert_eq!(pending.generation, generation);
                        pending.overrun = pending.overrun.saturating_add(expirations);
                        *pending
                    } else {
                        inner.next_episode = inner
                            .next_episode
                            .checked_add(1)
                            .expect("POSIX timer notification episode exhausted");
                        let pending = PendingEpisode {
                            generation,
                            episode: inner.next_episode,
                            overrun: expirations.saturating_sub(1),
                        };
                        inner.pending = Some(pending);
                        pending
                    };
                    Some(pending)
                },
            }
        };

        let Some(notification) = notification else {
            return;
        };
        let outcome = {
            let registration = self.signal_registration.lock();
            let Some(registration) = registration.as_ref() else {
                return;
            };
            registration.enqueue(
                notification.generation,
                notification.episode,
                clamp_overrun(notification.overrun),
            )
        };
        match outcome {
            PosixTimerSignalEnqueue::Queued | PosixTimerSignalEnqueue::AlreadyPending => {},
            PosixTimerSignalEnqueue::Ignored | PosixTimerSignalEnqueue::Consumed => {
                self.finish_unqueued_episode(notification);
            },
            PosixTimerSignalEnqueue::TargetExited => self.stop_after_target_exit(generation),
        }
    }

    fn finish_unqueued_episode(&self, expected: PendingEpisode) {
        let mut inner = self.inner.lock();
        if inner.deleted || inner.pending != Some(expected) {
            return;
        }
        inner.last_overrun = clamp_overrun(expected.overrun);
        inner.pending = None;
    }

    fn signal_delivered(&self, identity: PosixTimerSignalIdentity) {
        let mut inner = self.inner.lock();
        if inner.deleted || inner.generation != identity.generation() {
            return;
        }
        let Some(pending) = inner.pending else {
            return;
        };
        if pending.episode != identity.episode() || identity.timer_id() != self.id {
            return;
        }
        inner.last_overrun = clamp_overrun(pending.overrun);
        inner.pending = None;
    }

    fn stop_after_target_exit(&self, generation: u64) {
        let request = {
            let mut inner = self.inner.lock();
            if inner.deleted || inner.generation != generation {
                return;
            }
            inner.pending = None;
            let request = inner.arm.as_mut().and_then(|arm| arm.request.take());
            inner.arm = None;
            request
        };
        if let Some(request) = request {
            cancel_timer_event(&request);
        }
    }

    fn delete(&self) {
        let request = {
            let mut inner = self.inner.lock();
            if inner.deleted {
                return;
            }
            inner.deleted = true;
            inner.generation = inner
                .generation
                .checked_add(1)
                .expect("POSIX timer generation exhausted during delete");
            inner.pending = None;
            let request = inner.arm.as_mut().and_then(|arm| arm.request.take());
            inner.arm = None;
            request
        };
        if let Some(request) = request {
            cancel_timer_event(&request);
        }
        // Registration withdrawal follows object invalidation and request
        // cancellation. A signal occurrence already pending keeps its own
        // callback Arc and may finish later without rearming this object.
        drop(self.signal_registration.lock().take());
    }
}

impl ThreadGroup {
    pub(crate) fn prepare_posix_timer(
        self: &Arc<Self>,
        clock: PosixTimerClock,
        notification: PosixTimerNotification,
    ) -> Result<PreparedPosixTimer, SysError> {
        let (id, reservation) = self.posix_timers.table.lock().reserve()?;
        let (kind, signal) = match notification {
            PosixTimerNotification::None => (NotificationKind::None, None),
            PosixTimerNotification::DefaultSignal => {
                (NotificationKind::Signal, Some((SigNo::SIGALRM, id as u64)))
            },
            PosixTimerNotification::Signal { no, sigval } => {
                (NotificationKind::Signal, Some((no, sigval)))
            },
        };
        let timer = Arc::new(PosixTimer::new(id, clock, kind));
        if let Some((no, sigval)) = signal {
            let weak_timer = Arc::downgrade(&timer);
            let callback: Arc<PosixTimerSignalCallback> = Arc::new(move |identity, _reason| {
                if let Some(timer) = weak_timer.upgrade() {
                    // The current shared SIGEV_SIGNAL route predates typed
                    // completion and preserves its existing dequeue/flush
                    // behavior in Gate 1. Gate 2 consumes the reason only for
                    // the new exact-task notification mode.
                    timer.signal_delivered(identity);
                }
            });
            match PosixTimerSignalRegistration::try_new(self, no, id, sigval, callback) {
                Ok(registration) => {
                    timer.signal_registration.lock().replace(registration);
                },
                Err(error) => {
                    self.posix_timers
                        .table
                        .lock()
                        .release_reservation(id, reservation);
                    return Err(error);
                },
            }
        }
        Ok(PreparedPosixTimer {
            owner: Arc::downgrade(self),
            id,
            reservation,
            timer: Some(timer),
        })
    }

    fn get_posix_timer(&self, id: i32) -> Result<Arc<PosixTimer>, SysError> {
        self.posix_timers
            .table
            .lock()
            .get(id)
            .ok_or(SysError::InvalidArgument)
    }

    pub(crate) fn posix_timer_gettime(&self, id: i32) -> Result<PosixTimerSetting, SysError> {
        Ok(self.get_posix_timer(id)?.setting_snapshot())
    }

    pub(crate) fn posix_timer_settime(
        &self,
        id: i32,
        setting: PosixTimerSetting,
        absolute: bool,
    ) -> Result<PosixTimerSetting, SysError> {
        self.get_posix_timer(id)?.settime(setting, absolute)
    }

    pub(crate) fn posix_timer_getoverrun(&self, id: i32) -> Result<i32, SysError> {
        self.get_posix_timer(id)?.getoverrun()
    }

    pub(crate) fn delete_posix_timer(&self, id: i32) -> Result<(), SysError> {
        // ID removal is the syscall visibility linearization point. Object
        // generation and physical queue cancellation happen only afterwards.
        let timer = self
            .posix_timers
            .table
            .lock()
            .remove(id)
            .ok_or(SysError::InvalidArgument)?;
        timer.delete();
        Ok(())
    }

    pub(in crate::task) fn delete_all_posix_timers(&self) {
        let slots = self.posix_timers.table.lock().take_all();
        delete_slots(slots);
    }
}

fn delete_slots(slots: Vec<Option<PosixTimerSlot>>) {
    for slot in slots {
        if let Some(PosixTimerSlot::Published(timer)) = slot {
            timer.delete();
        }
    }
}

fn setting_snapshot(inner: &PosixTimerInner) -> PosixTimerSetting {
    let Some(arm) = &inner.arm else {
        return PosixTimerSetting::default();
    };
    let now_ns = arm.timeline.now_ns();
    let value_ns = if now_ns < arm.target_ns {
        arm.target_ns - now_ns
    } else if arm.interval_ns == 0 {
        0
    } else {
        let periods = periods_through(arm.target_ns, arm.interval_ns, now_ns);
        advance_target(arm.target_ns, arm.interval_ns, periods)
            .map(|target| target.saturating_sub(now_ns))
            .unwrap_or(0)
    };
    PosixTimerSetting {
        value_ns,
        interval_ns: arm.interval_ns,
    }
}

fn periods_through(target_ns: u64, interval_ns: u64, now_ns: u64) -> u64 {
    assert!(interval_ns != 0);
    assert!(now_ns >= target_ns);
    (now_ns - target_ns) / interval_ns + 1
}

fn advance_target(target_ns: u64, interval_ns: u64, periods: u64) -> Option<u64> {
    let delta = (interval_ns as u128).checked_mul(periods as u128)?;
    let next = (target_ns as u128).checked_add(delta)?;
    u64::try_from(next).ok()
}

fn clamp_overrun(overrun: u64) -> i32 {
    i32::try_from(overrun).unwrap_or(i32::MAX)
}

fn ns_to_duration(ns: u64) -> Duration {
    Duration::from_secs(ns / NSEC_PER_SEC) + Duration::from_nanos(ns % NSEC_PER_SEC)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::time::timer::queued_timer_count;

    #[kunit]
    fn timer_id_reservation_is_invisible_and_reusable() {
        let mut table = PosixTimerTable::default();
        let (first, first_token) = table.reserve().unwrap();
        let (second, _) = table.reserve().unwrap();
        assert_eq!((first, second), (0, 1));
        assert!(table.get(first).is_none());
        table.release_reservation(first, first_token);
        assert_eq!(table.reserve().unwrap().0, first);
    }

    #[kunit]
    fn stale_reservation_cannot_release_or_publish_reused_id() {
        let mut table = PosixTimerTable::default();
        let (id, stale_token) = table.reserve().unwrap();
        table.take_all();
        let (reused_id, current_token) = table.reserve().unwrap();
        assert_eq!(reused_id, id);
        assert_ne!(current_token, stale_token);

        table.release_reservation(id, stale_token);
        assert!(matches!(
            table.slots[id as usize],
            Some(PosixTimerSlot::Reserved(token)) if token == current_token
        ));
        let stale_timer = Arc::new(PosixTimer::new(
            id,
            PosixTimerClock::Monotonic,
            NotificationKind::None,
        ));
        assert_eq!(
            table.publish(id, stale_token, stale_timer),
            Err(SysError::NoSuchProcess)
        );
    }

    #[kunit]
    fn replace_disarm_delete_and_bulk_cleanup_return_queue_to_baseline() {
        let owner = get_current_task().get_thread_group();
        owner.delete_all_posix_timers();
        let cpu = cur_cpu_id();
        let baseline = queued_timer_count(cpu);

        let prepared = owner
            .prepare_posix_timer(PosixTimerClock::Monotonic, PosixTimerNotification::None)
            .unwrap();
        let id = prepared.id();
        prepared.publish().unwrap();
        for seconds in 1..=64 {
            owner
                .posix_timer_settime(
                    id,
                    PosixTimerSetting {
                        value_ns: (3600 + seconds) * NSEC_PER_SEC,
                        interval_ns: 0,
                    },
                    false,
                )
                .unwrap();
            assert_eq!(queued_timer_count(cpu), baseline + 1);
        }
        owner
            .posix_timer_settime(id, PosixTimerSetting::default(), false)
            .unwrap();
        assert_eq!(queued_timer_count(cpu), baseline);
        owner.delete_posix_timer(id).unwrap();

        for _ in 0..2 {
            let prepared = owner
                .prepare_posix_timer(PosixTimerClock::Monotonic, PosixTimerNotification::None)
                .unwrap();
            let id = prepared.id();
            prepared.publish().unwrap();
            owner
                .posix_timer_settime(
                    id,
                    PosixTimerSetting {
                        value_ns: 3600 * NSEC_PER_SEC,
                        interval_ns: 0,
                    },
                    false,
                )
                .unwrap();
        }
        assert_eq!(queued_timer_count(cpu), baseline + 2);
        owner.delete_all_posix_timers();
        assert_eq!(queued_timer_count(cpu), baseline);
        assert!(owner.posix_timer_gettime(0).is_err());
        assert!(owner.posix_timer_gettime(1).is_err());
    }

    #[kunit]
    fn periodic_advance_uses_original_target_and_clamps_overrun() {
        assert_eq!(periods_through(100, 10, 100), 1);
        assert_eq!(periods_through(100, 10, 139), 4);
        assert_eq!(advance_target(100, 10, 4), Some(140));
        assert_eq!(clamp_overrun(i32::MAX as u64 + 1), i32::MAX);
    }

    #[kunit]
    fn snapshot_derives_future_period_without_mutating_schedule() {
        let inner = PosixTimerInner {
            generation: 1,
            deleted: false,
            arm: Some(PosixTimerArm {
                timeline: PosixTimerTimeline::Monotonic,
                target_ns: 100,
                interval_ns: 10,
                request: None,
            }),
            next_episode: 0,
            pending: None,
            last_overrun: 0,
        };
        assert_eq!(
            setting_snapshot_at(&inner, 139),
            PosixTimerSetting {
                value_ns: 1,
                interval_ns: 10,
            }
        );
        assert_eq!(inner.arm.as_ref().unwrap().target_ns, 100);
    }

    fn setting_snapshot_at(inner: &PosixTimerInner, now_ns: u64) -> PosixTimerSetting {
        let arm = inner.arm.as_ref().unwrap();
        let value_ns = if now_ns < arm.target_ns {
            arm.target_ns - now_ns
        } else {
            let periods = periods_through(arm.target_ns, arm.interval_ns, now_ns);
            advance_target(arm.target_ns, arm.interval_ns, periods)
                .unwrap()
                .saturating_sub(now_ns)
        };
        PosixTimerSetting {
            value_ns,
            interval_ns: arm.interval_ns,
        }
    }
}
