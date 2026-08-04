use crate::{
    prelude::*,
    task::sig::{
        SigNo, Signal,
        info::{SiCode, SigInfoFields, SigTimer},
    },
    time::{
        duration_to_mono,
        timer::{TimerHandle, cancel_timer_event, schedule_threaded_timer_event},
    },
};

/// Real itimer state still uses a no-IRQ lock because schedule/cancel/snapshot
/// can race with stale timer completions. Threaded completions may take this
/// lock, but signal delivery must be committed under the lock and executed
/// after releasing it.
#[derive(Debug)]
pub struct ITimers {
    real: NoIrqSpinLock<Option<RealITimer>>,
    // TODO: virtual, prof.
}

#[derive(Debug)]
pub struct RealITimer {
    /// Authoritative target for getitimer and periodic advancement. Callback
    /// execution time must not replace it or periodic delivery would drift.
    expire_at: Instant,
    /// If [Some], then this is a periodic timer.
    interval: Option<Duration>,
    /// If false, then a stale timer completion will not send a signal to the
    /// thread group. Pointer identity also distinguishes replacement arms.
    validness: Arc<AtomicBool>,
    /// Cancellation capability for the current one-shot queue request. The
    /// handle is not the owner of the long-lived itimer schedule.
    request: Option<TimerHandle>,
}

impl ITimers {
    pub const fn new() -> Self {
        Self {
            real: NoIrqSpinLock::new(None),
        }
    }
}

impl Drop for ITimers {
    fn drop(&mut self) {
        // `TimerHandle` cannot cancel on Drop because fire-and-forget requests
        // are valid timer-core users. The itimer owner must first close callback
        // eligibility, withdraw its state, and only then cancel outside its lock.
        let request = {
            let mut real = self.real.lock();
            let request = real.as_mut().and_then(|timer| {
                timer.validness.store(false, Ordering::SeqCst);
                timer.request.take()
            });
            *real = None;
            request
        };
        if let Some(request) = request {
            cancel_timer_event(&request);
        }
    }
}

impl ThreadGroup {
    /// Set a real itimer. If the thread group already has a real itimer, then
    /// the old one will be cancelled and replaced by the new one.
    pub fn set_real_itimer(self: &Arc<Self>, timeout: Duration, interval: Option<Duration>) {
        assert_ne!(
            timeout,
            Duration::ZERO,
            "this should be checked by syscall handler"
        );

        let new_validness = Arc::new(AtomicBool::new(true));
        let callback_validness = new_validness.clone();
        let tg = Arc::downgrade(self);
        let old_request = {
            let mut real = self.itimers.real.lock();
            // Replacement is a two-part protocol: reject any completion that
            // already left the queue, then physically remove what is still
            // queue-owned after the new arm has been committed.
            let old_request = real.as_mut().and_then(|timer| {
                timer.validness.store(false, Ordering::SeqCst);
                timer.request.take()
            });

            // Submit before unlocking so the armed state is not visible without
            // a matching queued request.
            let expire_at = Instant::now() + timeout;
            let request = schedule_real_itimer_callback(tg, callback_validness, timeout);
            real.replace(RealITimer {
                expire_at,
                interval,
                validness: new_validness,
                request: Some(request),
            });
            old_request
        };
        if let Some(request) = old_request {
            cancel_timer_event(&request);
        }
    }

    pub fn cancel_real_itimer(&self) {
        let request = {
            let mut real = self.itimers.real.lock();
            let request = real.as_mut().and_then(|timer| {
                timer.validness.store(false, Ordering::SeqCst);
                timer.request.take()
            });
            *real = None;
            request
        };
        if let Some(request) = request {
            cancel_timer_event(&request);
        }
    }

    /// Returns (remaining time, optional interval) if the thread group has a
    /// real itimer, or [None] if it doesn't.
    pub fn real_itimer_snapshot(&self) -> Option<(Duration, Option<Duration>)> {
        let real = self.itimers.real.lock();
        if let Some(real) = real.as_ref() {
            let rem = real.expire_at.saturating_duration_since(Instant::now());
            Some((rem, real.interval))
        } else {
            None
        }
    }
}

fn schedule_real_itimer_callback(
    tg: Weak<ThreadGroup>,
    validness: Arc<AtomicBool>,
    timeout: Duration,
) -> TimerHandle {
    // ITIMER_REAL submits a bounded threaded completion, not a background job.
    // The thread-group itimer state keeps ownership of stale filtering,
    // interval rearm, and the signal action commit point.
    schedule_threaded_timer_event(
        timeout,
        Box::new(move || {
            if let Some(tg) = tg.upgrade() {
                real_itimer_expire_callback(tg, validness);
            }
        }),
    )
}

fn next_periodic_expiration(expire_at: Instant, interval: Duration, now: Instant) -> Instant {
    assert!(
        now >= expire_at,
        "ITIMER_REAL completion ran before its owner deadline"
    );
    let interval_mono = duration_to_mono(interval)
        .filter(|interval| *interval != 0)
        .expect("ITIMER_REAL interval is below the architecture clock resolution");
    let elapsed = now.mono() - expire_at.mono();
    // Skip every elapsed period in one step. Rearming from `now` would turn
    // timer-worker delay into permanent phase drift.
    let periods = elapsed / interval_mono + 1;
    let advance = interval_mono
        .checked_mul(periods)
        .expect("ITIMER_REAL periodic deadline overflow");
    Instant::from_mono(
        expire_at
            .mono()
            .checked_add(advance)
            .expect("ITIMER_REAL periodic deadline overflow"),
    )
}

fn real_itimer_expire_callback(tg: Arc<ThreadGroup>, validness: Arc<AtomicBool>) {
    let signal_committed = {
        let mut real = tg.itimers.real.lock();
        let Some(timer) = real.as_mut() else {
            return;
        };
        // Value alone is insufficient: a replacement owns a distinct Arc so a
        // stale callback can never match a newly-valid arm accidentally.
        if !Arc::ptr_eq(&timer.validness, &validness) || !validness.load(Ordering::SeqCst) {
            return;
        }
        assert!(
            timer.request.take().is_some(),
            "current ITIMER_REAL callback is missing its dequeued request handle"
        );

        match timer.interval {
            Some(interval) => {
                let now = Instant::now();
                timer.expire_at = next_periodic_expiration(timer.expire_at, interval, now);
                let timeout = timer.expire_at.saturating_duration_since(now);
                timer.request = Some(schedule_real_itimer_callback(
                    Arc::downgrade(&tg),
                    validness.clone(),
                    timeout,
                ));
            },
            None => {
                timer.validness.store(false, Ordering::SeqCst);
                *real = None;
            },
        }

        // Commit signal eligibility while the itimer state is stable, but send
        // after unlock because signal delivery crosses into another owner.
        true
    };

    if signal_committed {
        tg.recv_signal(real_itimer_signal());
    }
}

fn real_itimer_signal() -> Signal {
    Signal::new(
        SigNo::SIGALRM,
        SiCode::Timer,
        // Stub values for now; this stage only migrates completion context.
        SigInfoFields::Timer(SigTimer {
            tid: 0,
            overrun: 0,
            sigval: 0,
            sys_private: 0,
        }),
    )
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::time::timer::{queued_timer_count, timer_event_is_queued};

    #[kunit]
    fn replace_cancel_and_stale_completion_keep_one_live_request() {
        let tg = get_current_task().get_thread_group();
        tg.cancel_real_itimer();
        let cpu = cur_cpu_id();
        let baseline = queued_timer_count(cpu);

        tg.set_real_itimer(Duration::from_secs(3600), None);
        let stale_validness = {
            let real = tg.itimers.real.lock();
            let timer = real.as_ref().unwrap();
            assert!(timer_event_is_queued(timer.request.as_ref().unwrap()));
            timer.validness.clone()
        };
        assert_eq!(queued_timer_count(cpu), baseline + 1);

        tg.set_real_itimer(Duration::from_secs(1800), Some(Duration::from_secs(2)));
        assert_eq!(queued_timer_count(cpu), baseline + 1);
        real_itimer_expire_callback(tg.clone(), stale_validness);
        assert_eq!(queued_timer_count(cpu), baseline + 1);

        tg.cancel_real_itimer();
        assert_eq!(queued_timer_count(cpu), baseline);
        assert!(tg.real_itimer_snapshot().is_none());
    }

    #[kunit]
    fn itimer_owner_drop_removes_a_far_future_request() {
        let cpu = cur_cpu_id();
        let baseline = queued_timer_count(cpu);
        let request = schedule_threaded_timer_event(Duration::from_secs(3600), Box::new(|| {}));
        let timers = ITimers {
            real: NoIrqSpinLock::new(Some(RealITimer {
                expire_at: Instant::now() + Duration::from_secs(3600),
                interval: None,
                validness: Arc::new(AtomicBool::new(true)),
                request: Some(request),
            })),
        };
        assert_eq!(queued_timer_count(cpu), baseline + 1);
        drop(timers);
        assert_eq!(queued_timer_count(cpu), baseline);
    }

    #[kunit]
    fn delayed_periodic_completion_advances_from_the_old_target() {
        let interval = Duration::from_millis(10);
        let interval_mono = duration_to_mono(interval).unwrap();
        assert_ne!(interval_mono, 0);
        let expire_at = Instant::from_mono(100);
        let now = Instant::from_mono(100 + interval_mono * 3 + interval_mono / 2);
        let next = next_periodic_expiration(expire_at, interval, now);
        assert_eq!(next.mono(), 100 + interval_mono * 4);
        assert!(next > now);
    }

    #[kunit]
    fn repeated_itimer_replace_cancel_returns_to_baseline() {
        let tg = get_current_task().get_thread_group();
        tg.cancel_real_itimer();
        let cpu = cur_cpu_id();
        let baseline = queued_timer_count(cpu);
        for seconds in 1..=64 {
            tg.set_real_itimer(Duration::from_secs(3600 + seconds), None);
            assert_eq!(queued_timer_count(cpu), baseline + 1);
        }
        tg.cancel_real_itimer();
        assert_eq!(queued_timer_count(cpu), baseline);
    }
}
