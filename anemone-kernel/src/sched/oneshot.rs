//! Dormant single-producer, single-consumer value channel for scheduler work.
//!
//! The channel phase owns only payload and endpoint lifecycle. A receive-local
//! [`Latch`] owns each actual wait round; its outcome is never used as payload
//! truth. This separation lets a sender complete in hardirq context before a
//! receiver starts waiting, while wait-core `Force` only retires and rearms the
//! current round.

use core::mem;

use crate::prelude::*;

/// Create a dormant one-shot channel.
///
/// Construction allocates only the shared channel phase. It does not inspect
/// the current task or publish a wait round.
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Shared {
        state: NoIrqSpinLock::new(ChannelState {
            phase: Phase::Empty,
            trigger: None,
        }),
    });
    kdebugln!("sched oneshot: create channel={:#x}", shared.debug_id());

    (
        Sender {
            shared: Some(shared.clone()),
        },
        Receiver {
            shared: Some(shared),
        },
    )
}

/// The unique producer endpoint.
///
/// This type deliberately does not implement [`Clone`]. Consuming `send()` or
/// dropping the endpoint permanently retires the producer capability.
pub struct Sender<T> {
    shared: Option<Arc<Shared<T>>>,
}

impl<T> Sender<T> {
    /// Publish `value` exactly once without waiting for the receiver.
    ///
    /// If the receiver was already dropped, ownership of `value` is returned.
    /// A registered latch trigger is detached under the channel lock and fired
    /// only after that lock has been released.
    pub fn send(mut self, value: T) -> Result<(), T> {
        let shared = self
            .shared
            .take()
            .expect("sched oneshot sender capability already consumed");
        let channel_id = shared.debug_id();

        let (result, trigger) = {
            let mut state = shared.state.lock();
            match state.phase {
                Phase::Empty => {
                    state.phase = Phase::Value(value);
                    (Ok(()), state.trigger.take())
                },
                Phase::ReceiverClosed => {
                    assert!(
                        state.trigger.is_none(),
                        "sched oneshot channel {:#x} closed receiver retained a trigger",
                        channel_id,
                    );
                    state.phase = Phase::Consumed;
                    (Err(value), None)
                },
                _ => panic!(
                    "sched oneshot channel {:#x} sender observed invalid phase {}",
                    channel_id,
                    state.phase.name(),
                ),
            }
        };

        kdebugln!(
            "sched oneshot: send channel={:#x} result={} registered={}",
            channel_id,
            if result.is_ok() {
                "published"
            } else {
                "receiver_closed"
            },
            trigger.is_some(),
        );
        if let Some(trigger) = trigger {
            trigger.trigger();
        }
        result
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let Some(shared) = self.shared.take() else {
            return;
        };
        let channel_id = shared.debug_id();

        let (valid, wake, trigger, payload) = {
            let mut state = shared.state.lock();
            let phase = mem::replace(&mut state.phase, Phase::Consumed);
            let trigger = state.trigger.take();
            match phase {
                Phase::Empty => {
                    state.phase = Phase::SenderClosed;
                    (true, true, trigger, None)
                },
                Phase::ReceiverClosed => {
                    let valid = trigger.is_none();
                    (valid, false, trigger, None)
                },
                Phase::Value(value) => (false, false, trigger, Some(value)),
                Phase::SenderClosed | Phase::Consumed => (false, false, trigger, None),
            }
        };

        kdebugln!(
            "sched oneshot: close sender channel={:#x} registered={}",
            channel_id,
            trigger.is_some(),
        );
        if wake {
            if let Some(trigger) = trigger {
                trigger.trigger();
            }
        } else {
            drop(trigger);
        }
        drop(payload);
        assert!(
            valid,
            "sched oneshot channel {:#x} sender drop observed invalid phase",
            channel_id,
        );
    }
}

/// The unique consumer endpoint.
///
/// The endpoint may move before `recv_uninterruptible()` is called, but the
/// receive-local latch is always bound to the task that actually calls it.
pub struct Receiver<T> {
    shared: Option<Arc<Shared<T>>>,
}

impl<T> Receiver<T> {
    /// Wait until the producer publishes a value or permanently closes.
    ///
    /// Ordinary signals do not complete this operation. Wait-core `Force`
    /// retires only the current latch round; an empty channel is rearmed inside
    /// this method until a persistent terminal phase becomes observable.
    pub fn recv_uninterruptible(self) -> Result<T, RecvError> {
        self.recv_with_hook(|_| {})
    }

    fn recv_with_hook<F>(mut self, mut hook: F) -> Result<T, RecvError>
    where
        F: FnMut(ReceivePoint),
    {
        let shared = self
            .shared
            .take()
            .expect("sched oneshot receiver capability already consumed");
        let channel_id = shared.debug_id();

        loop {
            if let Some(terminal) = shared.take_terminal() {
                kdebugln!(
                    "sched oneshot: terminal fast path channel={:#x} result={}",
                    channel_id,
                    terminal_name(&terminal),
                );
                return terminal;
            }

            let latch = Latch::begin_current(false);
            hook(ReceivePoint::AfterBegin);
            let mut trigger = Some(latch.make_trigger());
            let wait_id = trigger
                .as_ref()
                .expect("sched oneshot receive trigger disappeared before registration")
                .wait_id();
            kdebugln!(
                "sched oneshot: begin receive round channel={:#x} wait={:#x}",
                channel_id,
                wait_id,
            );

            hook(ReceivePoint::BeforeRegister);
            let registered = {
                let mut state = shared.state.lock();
                match state.phase {
                    Phase::Empty => {
                        assert!(
                            state.trigger.is_none(),
                            "sched oneshot channel {:#x} installed a second receive trigger",
                            channel_id,
                        );
                        state.trigger = trigger.take();
                        true
                    },
                    Phase::Value(_) | Phase::SenderClosed => {
                        assert!(
                            state.trigger.is_none(),
                            "sched oneshot channel {:#x} terminal phase retained a trigger",
                            channel_id,
                        );
                        false
                    },
                    _ => panic!(
                        "sched oneshot channel {:#x} receiver registration observed invalid phase {}",
                        channel_id,
                        state.phase.name()
                    ),
                }
            };

            // An uninstalled trigger can retain wait-core state. Release it
            // before cancellation/finish and never while the channel lock is held.
            drop(trigger);

            if !registered {
                latch.cancel(LatchCancelReason::PredicateReady);
                let outcome = latch.finish();
                kdebugln!(
                    "sched oneshot: terminal won registration channel={:#x} wait={:#x} outcome={:?}",
                    channel_id,
                    wait_id,
                    outcome,
                );
                return shared
                    .take_terminal()
                    .unwrap_or_else(|| {
                        panic!(
                            "sched oneshot channel {:#x} terminal phase disappeared after registration race",
                            channel_id,
                        )
                    });
            }

            hook(ReceivePoint::AfterRegister);
            kdebugln!(
                "sched oneshot: receive trigger registered channel={:#x} wait={:#x}",
                channel_id,
                wait_id,
            );
            hook(ReceivePoint::BeforePark);
            latch.schedule_with_timeout(None);
            hook(ReceivePoint::AfterWake);

            let detached = {
                let mut state = shared.state.lock();
                state.trigger.take()
            };
            // The producer may already have detached this trigger. Either way,
            // any remaining trigger must be dropped outside the channel lock.
            drop(detached);

            let outcome = latch.finish();
            kdebugln!(
                "sched oneshot: finish receive round channel={:#x} wait={:#x} outcome={:?}",
                channel_id,
                wait_id,
                outcome,
            );

            if let Some(terminal) = shared.take_terminal() {
                kdebugln!(
                    "sched oneshot: consume terminal channel={:#x} wait={:#x} result={}",
                    channel_id,
                    wait_id,
                    terminal_name(&terminal),
                );
                return terminal;
            }

            assert_eq!(
                outcome,
                LatchWaitOutcome::Force,
                "sched oneshot channel {:#x} empty after non-Force latch completion",
                channel_id,
            );
            kdebugln!(
                "sched oneshot: rearm after Force channel={:#x} wait={:#x}",
                channel_id,
                wait_id,
            );
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let Some(shared) = self.shared.take() else {
            return;
        };
        let channel_id = shared.debug_id();

        let (valid, trigger, payload) = {
            let mut state = shared.state.lock();
            let trigger = state.trigger.take();
            let phase = mem::replace(&mut state.phase, Phase::Consumed);
            let valid = trigger.is_none();
            match phase {
                Phase::Empty => {
                    state.phase = Phase::ReceiverClosed;
                    (valid, trigger, None)
                },
                Phase::Value(value) => (valid, trigger, Some(value)),
                Phase::SenderClosed => (valid, trigger, None),
                Phase::ReceiverClosed | Phase::Consumed => (false, trigger, None),
            }
        };

        // `T::drop` is unconstrained and must not run with IRQs disabled or the
        // channel lock held. Trigger cleanup also precedes the invariant check.
        drop(trigger);
        drop(payload);
        assert!(
            valid,
            "sched oneshot channel {:#x} receiver drop observed invalid phase",
            channel_id,
        );
        kdebugln!("sched oneshot: close receiver channel={:#x}", channel_id);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecvError {
    SenderClosed,
}

struct Shared<T> {
    state: NoIrqSpinLock<ChannelState<T>>,
}

impl<T> Shared<T> {
    fn debug_id(&self) -> usize {
        self as *const Self as usize
    }

    fn take_terminal(&self) -> Option<Result<T, RecvError>> {
        let channel_id = self.debug_id();
        let (mut terminal, trigger, valid) = {
            let mut state = self.state.lock();
            let phase = mem::replace(&mut state.phase, Phase::Consumed);
            let trigger = state.trigger.take();
            let valid = trigger.is_none();
            match phase {
                Phase::Empty => {
                    state.phase = Phase::Empty;
                    (None, trigger, valid)
                },
                Phase::Value(value) => (Some(Ok(value)), trigger, valid),
                Phase::SenderClosed => (Some(Err(RecvError::SenderClosed)), trigger, valid),
                Phase::ReceiverClosed | Phase::Consumed => (None, trigger, false),
            }
        };

        // A corrupted terminal state must release wait capability and payload
        // after the channel guard, before exposing the invariant failure.
        drop(trigger);
        if !valid {
            drop(terminal.take());
        }
        assert!(
            valid,
            "sched oneshot channel {:#x} terminal read observed invalid phase or trigger",
            channel_id,
        );
        terminal
    }
}

impl<T> Drop for Shared<T> {
    fn drop(&mut self) {
        let channel_id = self.debug_id();
        let (valid, payload, trigger) = {
            let mut state = self.state.lock();
            let phase = mem::replace(&mut state.phase, Phase::Consumed);
            let trigger = state.trigger.take();
            let (valid, payload) = match phase {
                Phase::Consumed => (trigger.is_none(), None),
                Phase::Value(value) => (false, Some(value)),
                _ => (false, None),
            };
            (valid, payload, trigger)
        };

        // Cleanup comes before the invariant assertion so a lifecycle bug does
        // not amplify into a leaked wait token or payload.
        drop(trigger);
        drop(payload);
        assert!(
            valid,
            "sched oneshot channel {:#x} shared state dropped before consumption",
            channel_id,
        );
    }
}

struct ChannelState<T> {
    phase: Phase<T>,
    /// Current receive-round wake capability only. This diagnostic/cleanup
    /// slot does not decide whether the task is waiting or the payload exists.
    trigger: Option<LatchTrigger>,
}

enum Phase<T> {
    Empty,
    Value(T),
    SenderClosed,
    ReceiverClosed,
    Consumed,
}

impl<T> Phase<T> {
    fn name(&self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Value(_) => "value",
            Self::SenderClosed => "sender_closed",
            Self::ReceiverClosed => "receiver_closed",
            Self::Consumed => "consumed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReceivePoint {
    AfterBegin,
    BeforeRegister,
    AfterRegister,
    BeforePark,
    AfterWake,
}

fn terminal_name<T>(terminal: &Result<T, RecvError>) -> &'static str {
    match terminal {
        Ok(_) => "value",
        Err(RecvError::SenderClosed) => "sender_closed",
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{
        task::kthread::{KThreadBuilder, KThreadCtx},
        utils::any_opaque::AnyOpaque,
    };

    #[kunit]
    fn test_sched_oneshot_dormant_and_endpoint_paths() {
        assert!(get_current_task().is_sched_runnable());
        let (sender, receiver) = channel::<u32>();
        assert!(get_current_task().is_sched_runnable());
        assert_send::<Sender<u32>>();
        assert_send::<Receiver<u32>>();
        sender.send(7).expect("receiver is open");
        assert_eq!(receiver.recv_uninterruptible(), Ok(7));

        let (sender, receiver) = channel::<u32>();
        drop(sender);
        assert_eq!(
            receiver.recv_uninterruptible(),
            Err(RecvError::SenderClosed)
        );

        let (sender, receiver) = channel::<u32>();
        drop(receiver);
        assert_eq!(sender.send(11), Err(11));
    }

    #[kunit]
    fn test_sched_oneshot_send_registration_windows() {
        assert_send_at(ReceivePoint::AfterBegin, 13);
        assert_send_at(ReceivePoint::BeforeRegister, 17);
        assert_send_at(ReceivePoint::AfterRegister, 19);
        assert_send_at(ReceivePoint::BeforePark, 23);
    }

    #[kunit]
    fn test_sched_oneshot_force_windows_rearm_to_terminal() {
        for point in [
            ReceivePoint::AfterBegin,
            ReceivePoint::BeforeRegister,
            ReceivePoint::AfterRegister,
            ReceivePoint::BeforePark,
        ] {
            let (sender, receiver) = channel::<u32>();
            let mut sender = Some(sender);
            let mut forces = 0;
            let result = receiver.recv_with_hook(|observed| {
                if observed != point {
                    return;
                }
                if forces < 2 {
                    forces += 1;
                    notify(&get_current_task(), true);
                } else if let Some(sender) = sender.take() {
                    sender.send(29).expect("receiver remains open after Force");
                }
            });
            assert_eq!(forces, 2);
            assert_eq!(result, Ok(29));
        }
    }

    #[kunit]
    fn test_sched_oneshot_force_then_sender_close_and_terminal_race() {
        let (sender, receiver) = channel::<u32>();
        let mut sender = Some(sender);
        let mut forced = false;
        let result = receiver.recv_with_hook(|point| {
            if point != ReceivePoint::AfterRegister {
                return;
            }
            if !forced {
                forced = true;
                notify(&get_current_task(), true);
            } else {
                drop(sender.take());
            }
        });
        assert_eq!(result, Err(RecvError::SenderClosed));

        let (sender, receiver) = channel::<u32>();
        let mut sender = Some(sender);
        let mut competed = false;
        let result = receiver.recv_with_hook(|point| {
            if point == ReceivePoint::AfterRegister && !competed {
                competed = true;
                notify(&get_current_task(), true);
                sender
                    .take()
                    .expect("missing terminal-race sender")
                    .send(31)
                    .expect("receiver remains open during Force race");
            }
        });
        assert_eq!(result, Ok(31));
    }

    #[kunit]
    fn test_sched_oneshot_force_from_parked_round_rearms() {
        let (sender, receiver) = channel::<u32>();
        let target = get_current_task();
        let cpu = target.cpuid();
        let worker = KThreadBuilder::new("oneshot-force-park")
            .cpu(cpu)
            .spawn(
                force_parked_round_then_send,
                AnyOpaque::new(ParkedForceContext {
                    target,
                    sender: Some(sender),
                }),
            )
            .expect("failed to spawn parked Force worker");

        let mut rounds = 0;
        let result = receiver.recv_with_hook(|point| {
            if point == ReceivePoint::AfterBegin {
                rounds += 1;
            }
        });
        assert_eq!(result, Ok(37));
        assert!(rounds >= 2, "parked Force did not rearm the receiver");
        assert_eq!(worker.wait_exited(), 0);
    }

    #[kunit]
    fn test_sched_oneshot_payload_drop_is_exactly_once() {
        let drops = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = channel();
        sender
            .send(DropProbe(drops.clone()))
            .expect("receiver is open");
        drop(receiver);
        assert_eq!(drops.load(Ordering::Acquire), 1);

        let (sender, receiver) = channel();
        sender
            .send(DropProbe(drops.clone()))
            .expect("receiver is open");
        let value = receiver
            .recv_uninterruptible()
            .expect("published payload missing");
        assert_eq!(drops.load(Ordering::Acquire), 1);
        drop(value);
        assert_eq!(drops.load(Ordering::Acquire), 2);
    }

    fn assert_send<T: Send>() {}

    fn assert_send_at(point: ReceivePoint, value: u32) {
        let (sender, receiver) = channel();
        let mut sender = Some(sender);
        let result = receiver.recv_with_hook(|observed| {
            if observed == point {
                if let Some(sender) = sender.take() {
                    sender.send(value).expect("receiver is open");
                }
            }
        });
        assert_eq!(result, Ok(value));
    }

    #[derive(Opaque)]
    struct ParkedForceContext {
        target: Arc<Task>,
        sender: Option<Sender<u32>>,
    }

    fn force_parked_round_then_send(_: KThreadCtx, mut opaque: AnyOpaque) -> i32 {
        let context = opaque
            .cast_mut::<ParkedForceContext>()
            .expect("invalid parked Force context");
        wait_until_parked(&context.target);
        let sender = context.sender.take().expect("missing parked Force sender");
        notify(&context.target, true);
        // Exit this helper so the forced receiver can run and rearm. The later
        // IRQ callback both proves Sender is hardirq-safe and supplies the
        // terminal value only after a distinct receive round has begun.
        unsafe {
            crate::time::timer::schedule_local_irq_timer_event(
                Duration::from_millis(50),
                Box::new(move || {
                    sender
                        .send(37)
                        .expect("receiver closed during parked Force test");
                }),
            );
        }
        0
    }

    fn wait_until_parked(target: &Task) {
        loop {
            if let TaskSchedState::Waiting {
                park: ParkState::Parked,
                ..
            } = target.sched_state()
            {
                return;
            }
            yield_now();
        }
    }

    #[derive(Debug)]
    struct DropProbe(Arc<AtomicUsize>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }
}
