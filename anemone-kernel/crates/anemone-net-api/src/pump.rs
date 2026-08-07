use crate::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recheck {
    Idle,
    Immediate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PumpOutcome {
    /// The Stack retains unfinished work. This is not by itself a scheduling
    /// predicate because external provider progress may still be required.
    pub work_remaining: bool,
    /// Whether another bounded pump can progress without a new provider edge
    /// or a future deadline becoming due.
    pub recheck: Recheck,
    /// The next protocol deadline, interpreted against the consumer's current
    /// monotonic time.
    pub next_deadline: Option<Instant>,
}
