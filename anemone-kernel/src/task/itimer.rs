use crate::{
    prelude::*,
    task::sig::{
        SigNo, Signal,
        info::{SiCode, SigInfoFields, SigTimer},
    },
    time::timer::schedule_local_irq_timer_event,
};

/// Each itimer must be accquired with irqsave to avoid deadlock with timer
/// interrupt handler.
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
    /// If false, then the on-fly timer interrupt handler won't send a signal to
    /// the thread group.
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

        let mut real = self.itimers.real.lock();
        if let Some(real) = real.as_mut() {
            // prevent stale timer from sending signals.
            real.validness.store(false, Ordering::SeqCst);
        }
        let new_validness = Arc::new(AtomicBool::new(true));
        real.replace(RealITimer {
            expire_at: Instant::now() + timeout,
            interval,
            validness: new_validness.clone(),
        });

        // NOTE: real's lock must not be dropped when schedule_local_irq_timer_event is
        // called!
        let tg = Arc::downgrade(self);
        unsafe {
            schedule_local_irq_timer_event(
                timeout,
                Box::new(move || {
                    if let Some(tg) = tg.upgrade() {
                        let mut real = tg.itimers.real.lock();
                        // validness must be checked after acquiring the lock.
                        if new_validness.load(Ordering::SeqCst) {
                            // still valid.
                            tg.recv_signal(Signal::new(
                                SigNo::SIGALRM,
                                SiCode::Timer,
                                // stub values for now.
                                SigInfoFields::Timer(SigTimer {
                                    tid: 0,
                                    overrun: 0,
                                    sigval: 0,
                                    sys_private: 0,
                                }),
                            ));

                            // rearm if it's a periodic timer.
                            drop(real); // avoid deadlock.
                            if let Some(interval) = interval {
                                tg.set_real_itimer(interval, Some(interval));
                            }
                        }
                    }
                }),
            );
        }
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
