use crate::{
    prelude::*,
    task::sig::{
        SigNo, Signal,
        info::{SiCode, SigInfoFields, SigTimer},
    },
    time::timer::schedule_threaded_timer_event,
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
    expire_at: Instant,
    /// If [Some], then this is a periodic timer.
    interval: Option<Duration>,
    /// If false, then a stale timer completion will not send a signal to the
    /// thread group.
    validness: Arc<AtomicBool>,
}

impl ITimers {
    pub const fn new() -> Self {
        Self {
            real: NoIrqSpinLock::new(None),
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
        let mut real = self.itimers.real.lock();
        if let Some(real) = real.as_mut() {
            // prevent stale timer from sending signals.
            real.validness.store(false, Ordering::SeqCst);
        }
        real.replace(RealITimer {
            expire_at: Instant::now() + timeout,
            interval,
            validness: new_validness.clone(),
        });

        // Submit before unlocking so the armed state is not visible without a
        // queued timer-core event. The threaded timer API has no recoverable
        // allocation failure path in this RFC stage.
        schedule_real_itimer_callback(tg, callback_validness, timeout);
    }

    pub fn cancel_real_itimer(&self) {
        let mut real = self.itimers.real.lock();
        if let Some(real) = real.as_mut() {
            // prevent stale timer from sending signals.
            real.validness.store(false, Ordering::SeqCst);
        }
        *real = None;
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
) {
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
    );
}

fn real_itimer_expire_callback(tg: Arc<ThreadGroup>, validness: Arc<AtomicBool>) {
    let signal_committed = {
        let mut real = tg.itimers.real.lock();
        let Some(timer) = real.as_mut() else {
            return;
        };
        if !Arc::ptr_eq(&timer.validness, &validness) || !validness.load(Ordering::SeqCst) {
            return;
        }

        match timer.interval {
            Some(interval) => {
                timer.expire_at = Instant::now() + interval;
                schedule_real_itimer_callback(Arc::downgrade(&tg), validness.clone(), interval);
            },
            None => {
                timer.validness.store(false, Ordering::SeqCst);
                *real = None;
            },
        }

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
