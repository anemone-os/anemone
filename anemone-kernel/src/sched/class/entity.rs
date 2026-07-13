use super::{fair::FairEntity, rt::RtEntity};

#[derive(Debug)]
pub struct SchedEntity {
    pub(super) on_runq: bool,
    pub(super) class: SchedClassPrv,
}

/// Capability for scheduler-class code that must mutate entity storage.
///
/// The type is visible to the task lock owner, but only scheduler-class
/// modules can construct it. This keeps ordinary crate callers from replacing
/// a published entity while still letting class transactions update membership
/// and class-owned accounting under the entity lock.
pub(crate) struct SchedEntityMutToken(());

impl SchedEntityMutToken {
    pub(super) const fn new() -> Self {
        Self(())
    }
}

impl SchedEntity {
    pub(super) fn new(class: SchedClassPrv) -> Self {
        Self {
            on_runq: false,
            class,
        }
    }

    /// Construct a fresh non-idle entity using the compile-time RT selector.
    ///
    /// The RT module owns policy selection and payload validation; this method
    /// is only the public `SchedEntity` facade that wraps that opaque payload.
    pub fn new_default() -> Self {
        Self::new(SchedClassPrv::Realtime(RtEntity::new_default()))
    }

    /// Construct the special idle entity.
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

#[derive(Debug)]
pub(super) enum SchedClassPrv {
    // The payload's policy and accounting invariants belong to its class
    // module; this enum only stores the opaque class-owned value.
    Realtime(RtEntity),
    Fair(FairEntity),
    Idle(()),
}

impl SchedClassPrv {
    const fn kind(&self) -> SchedClassKind {
        match self {
            Self::Realtime(_) => SchedClassKind::Realtime,
            Self::Fair(_) => SchedClassKind::Fair,
            Self::Idle(()) => SchedClassKind::Idle,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedClassKind {
    Realtime,
    Fair,
    Idle,
}
