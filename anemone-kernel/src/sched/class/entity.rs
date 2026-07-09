use crate::sched::class::eevdf::EevdfEntity;

#[derive(Debug, Clone)]
pub struct SchedEntity {
    pub(super) on_runq: bool,
    pub(super) class: SchedClassPrv,
}

impl SchedEntity {
    /// Create a new scheduling entity with the given scheduling class.
    pub fn new(class: SchedClassPrv) -> Self {
        Self {
            on_runq: false,
            class,
        }
    }

    /// Create a fresh entity for the current default normal scheduler.
    ///
    /// Checkpoint 2A deliberately keeps the default normal class on RR. The
    /// phase-3 default switch must flip this constructor instead of
    /// hand-writing EEVDF payloads at task creation call sites.
    pub fn new_normal() -> Self {
        Self::new(SchedClassPrv::RoundRobin(()))
    }

    /// Create a fresh entity for explicit EEVDF-directed tests or probes.
    ///
    /// This is not the default normal constructor until phase 3 closes the
    /// accounting, eligibility, and wake-placement gates.
    pub fn new_eevdf() -> Self {
        Self::new(SchedClassPrv::Eevdf(EevdfEntity::new()))
    }

    pub fn new_idle() -> Self {
        Self::new(SchedClassPrv::Idle(()))
    }

    /// **`on_runq` should never be accessed on a cpu which does not own the
    /// task. Correctness of scheduling system relies on this invariant.**
    pub fn on_runq(&self) -> bool {
        self.on_runq
    }

    pub fn class_kind(&self) -> SchedClassKind {
        self.class.kind()
    }

    pub(super) fn class(&self) -> &SchedClassPrv {
        &self.class
    }
}

#[derive(Debug, Clone)]
pub enum SchedClassPrv {
    Eevdf(EevdfEntity),
    // TODO: time slice.
    RoundRobin(()),
    Idle(()),
}

impl SchedClassPrv {
    pub const fn kind(&self) -> SchedClassKind {
        match self {
            Self::Eevdf(_) => SchedClassKind::Eevdf,
            Self::RoundRobin(()) => SchedClassKind::RoundRobin,
            Self::Idle(()) => SchedClassKind::Idle,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedClassKind {
    Eevdf,
    RoundRobin,
    Idle,
}
