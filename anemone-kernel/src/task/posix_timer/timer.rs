//! Per-object POSIX timer schedule, generation, and notification lifecycle.

use super::{NSEC_PER_SEC, PosixTimerClock, PosixTimerSetting};
use crate::{
    prelude::*,
    task::sig::{
        PosixTimerSignalCompletion, PosixTimerSignalEnqueue, PosixTimerSignalIdentity,
        PosixTimerSignalRegistration,
    },
    time::{
        RealtimeInstant, monotonic_ns, realtime_ns,
        timer::{
            TimerHandle, cancel_timer_event, schedule_realtime_threaded_timer_event,
            schedule_threaded_timer_event,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NotificationKind {
    None,
    SharedSignal,
    ThreadSignal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PosixTimerTimeline {
    Realtime,
    Monotonic,
}

impl PosixTimerClock {
    fn absolute_timeline(self) -> PosixTimerTimeline {
        match self {
            Self::Realtime => PosixTimerTimeline::Realtime,
            Self::Monotonic | Self::Boottime => PosixTimerTimeline::Monotonic,
        }
    }
}

impl PosixTimerTimeline {
    fn now_ns(self) -> u64 {
        match self {
            Self::Realtime => realtime_ns(),
            Self::Monotonic => monotonic_ns(),
        }
    }

    fn deadline(self, target_ns: u64) -> PosixTimerDeadline {
        match self {
            Self::Realtime => PosixTimerDeadline::Realtime(RealtimeInstant::from_nanos(target_ns)),
            Self::Monotonic => PosixTimerDeadline::Monotonic(target_ns),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PosixTimerDeadline {
    Realtime(RealtimeInstant),
    /// Exact logical nanoseconds used by timer_gettime and periodic accounting.
    /// This must not be replaced by the counter-rounded `MonotonicInstant`.
    Monotonic(u64),
}

impl PosixTimerDeadline {
    fn now_ns(self) -> u64 {
        match self {
            Self::Realtime(_) => realtime_ns(),
            Self::Monotonic(_) => monotonic_ns(),
        }
    }

    fn target_ns(self) -> u64 {
        match self {
            Self::Realtime(deadline) => deadline.as_nanos(),
            Self::Monotonic(deadline_ns) => deadline_ns,
        }
    }

    fn with_target_ns(self, target_ns: u64) -> Self {
        match self {
            Self::Realtime(_) => Self::Realtime(RealtimeInstant::from_nanos(target_ns)),
            Self::Monotonic(_) => Self::Monotonic(target_ns),
        }
    }
}

#[derive(Debug)]
pub(super) struct PosixTimer {
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
    /// Actual expirations ignored while SIGEV_THREAD_ID had SIG_IGN.
    /// This is accrual, not a delivered snapshot; a later real occurrence
    /// transfers it to that occurrence before dequeue commits it.
    ignored_expirations: u64,
}

#[derive(Debug)]
struct PosixTimerArm {
    /// Relative arms always use monotonic. Only an absolute CLOCK_REALTIME arm
    /// uses the mutable calendar timeline, so date changes cannot alter an
    /// elapsed-duration request.
    /// Authoritative target; callback execution time never
    /// replaces this value, so periodic timers cannot accumulate scheduler
    /// drift.
    deadline: PosixTimerDeadline,
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
    pub(super) fn new(id: i32, clock: PosixTimerClock, kind: NotificationKind) -> Self {
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
                ignored_expirations: 0,
            }),
        }
    }

    pub(super) fn install_signal_registration(&self, registration: PosixTimerSignalRegistration) {
        self.signal_registration.lock().replace(registration);
    }

    fn schedule(self: &Arc<Self>, generation: u64, deadline: PosixTimerDeadline) -> TimerHandle {
        let timer = Arc::downgrade(self);
        match deadline {
            PosixTimerDeadline::Realtime(deadline) => schedule_realtime_threaded_timer_event(
                deadline,
                None,
                Box::new(move || {
                    if let Some(timer) = timer.upgrade() {
                        timer.expire(generation);
                    }
                }),
                None,
            ),
            PosixTimerDeadline::Monotonic(deadline_ns) => {
                let delay = ns_to_duration(deadline_ns.saturating_sub(monotonic_ns()));
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

    pub(super) fn setting_snapshot(&self) -> PosixTimerSetting {
        let inner = self.inner.lock();
        setting_snapshot(&inner)
    }

    pub(super) fn settime(
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
            inner.ignored_expirations = 0;
            let old_request = inner.arm.as_mut().and_then(|arm| arm.request.take());
            inner.arm = new_target.map(|target_ns| {
                let deadline = timeline.deadline(target_ns);
                let request = self.schedule(inner.generation, deadline);
                PosixTimerArm {
                    deadline,
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

    pub(super) fn getoverrun(&self) -> Result<i32, SysError> {
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

            let now_ns = arm.deadline.now_ns();
            let target_ns = arm.deadline.target_ns();
            if now_ns < target_ns {
                arm.request = Some(self.schedule(generation, arm.deadline));
                inner.arm = Some(arm);
                return;
            }

            let expirations = if arm.interval_ns == 0 {
                1
            } else {
                periods_through(target_ns, arm.interval_ns, now_ns)
            };
            if arm.interval_ns != 0 {
                let Some(next_target) = advance_target(target_ns, arm.interval_ns, expirations)
                else {
                    // No representable future target remains. Disarm rather
                    // than creating a wrapped second schedule truth.
                    inner.arm = None;
                    return;
                };
                arm.deadline = arm.deadline.with_target_ns(next_target);
                if self.notification_kind != NotificationKind::ThreadSignal {
                    arm.request = Some(self.schedule(generation, arm.deadline));
                }
                inner.arm = Some(arm);
            }

            match self.notification_kind {
                NotificationKind::None => return,
                NotificationKind::SharedSignal | NotificationKind::ThreadSignal => {
                    let pending = if let Some(pending) = inner.pending.as_mut() {
                        assert_eq!(pending.generation, generation);
                        pending.overrun = pending.overrun.saturating_add(expirations);
                        *pending
                    } else {
                        inner.next_episode = inner
                            .next_episode
                            .checked_add(1)
                            .expect("POSIX timer notification episode exhausted");
                        let ignored_expirations =
                            if self.notification_kind == NotificationKind::ThreadSignal {
                                core::mem::take(&mut inner.ignored_expirations)
                            } else {
                                0
                            };
                        let pending = PendingEpisode {
                            generation,
                            episode: inner.next_episode,
                            overrun: ignored_expirations
                                .saturating_add(expirations.saturating_sub(1)),
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
            PosixTimerSignalEnqueue::Ignored => match self.notification_kind {
                NotificationKind::SharedSignal => self.finish_unqueued_episode(notification),
                NotificationKind::ThreadSignal => {
                    self.finish_thread_unqueued_episode(notification, false)
                },
                NotificationKind::None => unreachable!(),
            },
            PosixTimerSignalEnqueue::Consumed => match self.notification_kind {
                NotificationKind::SharedSignal => self.finish_unqueued_episode(notification),
                NotificationKind::ThreadSignal => {
                    self.finish_thread_unqueued_episode(notification, true)
                },
                NotificationKind::None => unreachable!(),
            },
            PosixTimerSignalEnqueue::TargetExited => match self.notification_kind {
                NotificationKind::SharedSignal => self.stop_after_target_exit(generation),
                NotificationKind::ThreadSignal => self.stop_after_thread_target_exit(generation),
                NotificationKind::None => unreachable!(),
            },
        }
    }

    fn finish_thread_unqueued_episode(self: &Arc<Self>, expected: PendingEpisode, commit: bool) {
        let mut inner = self.inner.lock();
        if inner.deleted || inner.pending != Some(expected) {
            return;
        }
        if commit {
            inner.last_overrun = clamp_overrun(expected.overrun);
        } else if self.notification_kind == NotificationKind::ThreadSignal {
            // `expected.overrun` excludes the expiry that created this
            // ignored episode. Accumulating actual expirations avoids storing
            // Linux's internal -1 baseline as a second state variable.
            inner.ignored_expirations = inner
                .ignored_expirations
                .saturating_add(expected.overrun.saturating_add(1));
        }
        inner.pending = None;
        self.rearm_thread_periodic_locked(&mut inner);
    }

    pub(super) fn thread_signal_completed(
        self: &Arc<Self>,
        identity: PosixTimerSignalIdentity,
        reason: PosixTimerSignalCompletion,
    ) -> Option<i32> {
        let mut inner = self.inner.lock();
        if inner.deleted || inner.generation != identity.generation() {
            return None;
        }
        let Some(pending) = inner.pending else {
            return None;
        };
        if pending.episode != identity.episode() || identity.timer_id() != self.id {
            return None;
        }

        if reason == PosixTimerSignalCompletion::Flushed {
            // Signal removed the occurrence without userspace consumption.
            // Keep only the periodic projection; a flush cannot commit overrun
            // or create the next physical request.
            inner.pending = None;
            return None;
        }

        let mut delivered_overrun = pending.overrun;
        let mut overflowed = false;
        if let Some(arm) = inner.arm.as_mut() {
            assert!(
                arm.request.is_none(),
                "thread-directed timer rearmed before signal dequeue"
            );
            if arm.interval_ns != 0 {
                let now_ns = arm.deadline.now_ns();
                let target_ns = arm.deadline.target_ns();
                if now_ns >= target_ns {
                    let periods = periods_through(target_ns, arm.interval_ns, now_ns);
                    delivered_overrun = delivered_overrun.saturating_add(periods);
                    if let Some(target_ns) = advance_target(target_ns, arm.interval_ns, periods) {
                        arm.deadline = arm.deadline.with_target_ns(target_ns);
                    } else {
                        overflowed = true;
                    }
                }
            }
        }
        if overflowed {
            inner.arm = None;
        }
        inner.last_overrun = clamp_overrun(delivered_overrun);
        inner.pending = None;
        self.rearm_thread_periodic_locked(&mut inner);
        Some(inner.last_overrun)
    }

    fn rearm_thread_periodic_locked(self: &Arc<Self>, inner: &mut PosixTimerInner) {
        let Some(arm) = inner.arm.as_mut() else {
            return;
        };
        if arm.interval_ns == 0 {
            return;
        }
        assert!(
            arm.request.is_none(),
            "thread-directed periodic timer already has a request"
        );
        arm.request = Some(self.schedule(inner.generation, arm.deadline));
    }

    fn finish_unqueued_episode(&self, expected: PendingEpisode) {
        let mut inner = self.inner.lock();
        if inner.deleted || inner.pending != Some(expected) {
            return;
        }
        inner.last_overrun = clamp_overrun(expected.overrun);
        inner.pending = None;
    }

    pub(super) fn signal_delivered(&self, identity: PosixTimerSignalIdentity) {
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

    fn stop_after_thread_target_exit(&self, generation: u64) {
        let mut inner = self.inner.lock();
        if inner.deleted || inner.generation != generation {
            return;
        }
        // A periodic arm remains as projection-only state. There is no physical
        // request after failed exact delivery, and a one-shot has no arm here.
        if let Some(arm) = inner.arm.as_ref() {
            assert!(
                arm.request.is_none(),
                "failed thread-directed delivery retained a physical request"
            );
        }
        inner.pending = None;
    }

    pub(super) fn delete(&self) {
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

fn setting_snapshot(inner: &PosixTimerInner) -> PosixTimerSetting {
    let Some(arm) = &inner.arm else {
        return PosixTimerSetting::default();
    };
    let now_ns = arm.deadline.now_ns();
    let target_ns = arm.deadline.target_ns();
    let value_ns = if now_ns < target_ns {
        target_ns - now_ns
    } else if arm.interval_ns == 0 {
        0
    } else {
        let periods = periods_through(target_ns, arm.interval_ns, now_ns);
        advance_target(target_ns, arm.interval_ns, periods)
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
    fn periodic_advance_uses_original_target_and_clamps_overrun() {
        assert_eq!(periods_through(100, 10, 100), 1);
        assert_eq!(periods_through(100, 10, 139), 4);
        assert_eq!(advance_target(100, 10, 4), Some(140));
        assert_eq!(clamp_overrun(i32::MAX as u64 + 1), i32::MAX);
    }

    #[kunit]
    fn thread_unqueued_outcomes_control_overrun_commit() {
        let cpu = cur_cpu_id();
        let baseline = queued_timer_count(cpu);
        let timer = Arc::new(PosixTimer::new(
            52,
            PosixTimerClock::Monotonic,
            NotificationKind::ThreadSignal,
        ));
        let target_ns = monotonic_ns().saturating_add(NSEC_PER_SEC);
        {
            let mut inner = timer.inner.lock();
            inner.generation = 6;
            inner.arm = Some(PosixTimerArm {
                deadline: PosixTimerDeadline::Monotonic(target_ns),
                interval_ns: NSEC_PER_SEC,
                request: None,
            });
            inner.pending = Some(PendingEpisode {
                generation: 6,
                episode: 9,
                overrun: 7,
            });
            inner.last_overrun = 2;
        }
        let ignored = timer.inner.lock().pending.unwrap();
        timer.finish_thread_unqueued_episode(ignored, false);
        let ignored_request = {
            let mut inner = timer.inner.lock();
            assert_eq!(inner.last_overrun, 2);
            assert_eq!(inner.ignored_expirations, 8);
            assert!(inner.arm.as_ref().unwrap().request.is_some());
            let request = inner.arm.as_mut().unwrap().request.take().unwrap();
            // The next block constructs a standalone Consumed episode instead
            // of passing through expiry, which would transfer this accrual.
            inner.ignored_expirations = 0;
            inner.pending = Some(PendingEpisode {
                generation: 6,
                episode: 10,
                overrun: 8,
            });
            request
        };
        assert!(cancel_timer_event(&ignored_request));
        let consumed = timer.inner.lock().pending.unwrap();
        timer.finish_thread_unqueued_episode(consumed, true);
        {
            let inner = timer.inner.lock();
            assert_eq!(inner.last_overrun, 8);
            assert!(inner.arm.as_ref().unwrap().request.is_some());
        }
        assert_eq!(queued_timer_count(cpu), baseline + 1);
        timer.delete();
        assert_eq!(queued_timer_count(cpu), baseline);
    }

    #[kunit]
    fn thread_target_exit_keeps_periodic_projection_only() {
        let cpu = cur_cpu_id();
        let baseline = queued_timer_count(cpu);
        let timer = Arc::new(PosixTimer::new(
            53,
            PosixTimerClock::Monotonic,
            NotificationKind::ThreadSignal,
        ));
        let now_ns = monotonic_ns();
        {
            let mut inner = timer.inner.lock();
            inner.generation = 8;
            inner.arm = Some(PosixTimerArm {
                deadline: PosixTimerDeadline::Monotonic(now_ns.saturating_sub(1)),
                interval_ns: NSEC_PER_SEC,
                request: None,
            });
            inner.pending = Some(PendingEpisode {
                generation: 8,
                episode: 11,
                overrun: 0,
            });
        }
        timer.stop_after_thread_target_exit(8);
        let setting = timer.setting_snapshot();
        assert_eq!(setting.interval_ns, NSEC_PER_SEC);
        assert!(setting.value_ns > 0);
        let inner = timer.inner.lock();
        assert!(inner.pending.is_none());
        assert!(inner.arm.as_ref().unwrap().request.is_none());
        assert_eq!(queued_timer_count(cpu), baseline);
    }

    #[kunit]
    fn snapshot_derives_future_period_without_mutating_schedule() {
        let inner = PosixTimerInner {
            generation: 1,
            deleted: false,
            arm: Some(PosixTimerArm {
                deadline: PosixTimerDeadline::Monotonic(100),
                interval_ns: 10,
                request: None,
            }),
            next_episode: 0,
            pending: None,
            last_overrun: 0,
            ignored_expirations: 0,
        };
        assert_eq!(
            setting_snapshot_at(&inner, 139),
            PosixTimerSetting {
                value_ns: 1,
                interval_ns: 10,
            }
        );
        assert_eq!(inner.arm.as_ref().unwrap().deadline.target_ns(), 100);
    }

    fn setting_snapshot_at(inner: &PosixTimerInner, now_ns: u64) -> PosixTimerSetting {
        let arm = inner.arm.as_ref().unwrap();
        let target_ns = arm.deadline.target_ns();
        let value_ns = if now_ns < target_ns {
            target_ns - now_ns
        } else {
            let periods = periods_through(target_ns, arm.interval_ns, now_ns);
            advance_target(target_ns, arm.interval_ns, periods)
                .unwrap()
                .saturating_sub(now_ns)
        };
        PosixTimerSetting {
            value_ns,
            interval_ns: arm.interval_ns,
        }
    }
}
