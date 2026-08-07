//! Pump budget, fair-order outcome, and shared recheck computation.

use anemone_net_api::{Instant, PumpOutcome, Recheck};

/// What, if anything, can advance unfinished work after the current round.
///
/// Provider-backed paths may wait for a durable provider edge. The software
/// local link has no such external owner and must classify retained work as
/// runnable instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RoundContinuation {
    Quiescent,
    Runnable,
    AwaitProviderEdge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PumpBudget {
    ingress_frames: usize,
    egress_steps: usize,
}

impl PumpBudget {
    pub const fn new(ingress_frames: usize, egress_steps: usize) -> Self {
        assert!(ingress_frames > 0, "ingress pump budget must be non-zero");
        assert!(egress_steps > 0, "egress pump budget must be non-zero");
        Self {
            ingress_frames,
            egress_steps,
        }
    }

    #[allow(dead_code)]
    pub(crate) const fn ingress_frames(self) -> usize {
        self.ingress_frames
    }

    #[allow(dead_code)]
    pub(crate) const fn egress_steps(self) -> usize {
        self.egress_steps
    }
}

pub(super) fn pump_outcome(
    continuation: RoundContinuation,
    now: Instant,
    next_deadline: Option<Instant>,
) -> PumpOutcome {
    let deadline_due = next_deadline.is_some_and(|deadline| deadline <= now);
    let awaiting_provider_edge = continuation == RoundContinuation::AwaitProviderEdge;
    let immediate = match continuation {
        RoundContinuation::Quiescent => deadline_due,
        RoundContinuation::Runnable => true,
        RoundContinuation::AwaitProviderEdge => false,
    };
    // A due protocol deadline cannot make progress while the provider owns a
    // blocking link/resource fact. Keeping that already-due deadline would
    // make the outer worker predicate immediately true again and busy-repoll.
    // A future deadline remains useful and may wake the worker once.
    let next_deadline = if awaiting_provider_edge && deadline_due {
        None
    } else {
        next_deadline
    };
    PumpOutcome {
        work_remaining: continuation != RoundContinuation::Quiescent || deadline_due,
        recheck: if immediate {
            Recheck::Immediate
        } else {
            Recheck::Idle
        },
        next_deadline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_distinguishes_runnable_work_from_provider_wait() {
        let future = Instant::from_micros(11);
        let before = pump_outcome(
            RoundContinuation::Quiescent,
            Instant::from_micros(10),
            Some(future),
        );
        assert!(!before.work_remaining);
        assert_eq!(before.recheck, Recheck::Idle);
        assert_eq!(before.next_deadline, Some(future));

        let runnable = pump_outcome(RoundContinuation::Runnable, Instant::from_micros(10), None);
        assert!(runnable.work_remaining);
        assert_eq!(runnable.recheck, Recheck::Immediate);

        let blocked_future = pump_outcome(
            RoundContinuation::AwaitProviderEdge,
            Instant::from_micros(10),
            Some(future),
        );
        assert!(blocked_future.work_remaining);
        assert_eq!(blocked_future.recheck, Recheck::Idle);
        assert_eq!(blocked_future.next_deadline, Some(future));

        let due = pump_outcome(RoundContinuation::Quiescent, future, Some(future));
        assert!(due.work_remaining);
        assert_eq!(due.recheck, Recheck::Immediate);

        let blocked = pump_outcome(RoundContinuation::AwaitProviderEdge, future, Some(future));
        assert!(blocked.work_remaining);
        assert_eq!(blocked.recheck, Recheck::Idle);
        assert_eq!(blocked.next_deadline, None);
    }
}
