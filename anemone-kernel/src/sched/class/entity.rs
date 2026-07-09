/// [Copy] is implemented cz we expect this struct should be a POD type.
#[derive(Debug, Clone, Copy)]
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

    /// **`on_runq` should never be accessed on a cpu which does not own the
    /// task. Correctness of scheduling system relies on this invariant.**
    pub fn on_runq(&self) -> bool {
        self.on_runq
    }

    pub(super) fn class(&self) -> SchedClassPrv {
        self.class
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SchedClassPrv {
    // TODO: time slice.
    RoundRobin(()),
    Idle(()),
}
