use crate::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recheck {
    Idle,
    Immediate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PumpOutcome {
    pub work_remaining: bool,
    pub recheck: Recheck,
    pub next_deadline: Option<Instant>,
}
