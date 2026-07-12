#[derive(Debug, Clone)]
pub struct SchedEntity {
    pub(super) on_runq: bool,
    pub(super) class: SchedClassPrv,
}

impl SchedEntity {
    /// Create a new scheduling entity with the given scheduling class.
    fn new(class: SchedClassPrv) -> Self {
        Self {
            on_runq: false,
            class,
        }
    }

    /// Create a fresh entity for the current default normal scheduler.
    pub fn new_normal() -> Self {
        Self::new(SchedClassPrv::RoundRobin(()))
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
}

#[derive(Debug, Clone)]
pub(super) enum SchedClassPrv {
    // TODO: time slice.
    RoundRobin(()),
    Idle(()),
}

impl SchedClassPrv {
    const fn kind(&self) -> SchedClassKind {
        match self {
            Self::RoundRobin(()) => SchedClassKind::RoundRobin,
            Self::Idle(()) => SchedClassKind::Idle,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedClassKind {
    RoundRobin,
    Idle,
}
