//! Nemophila integration owned by the task clone subsystem.

use crate::{
    nemophila::weave::{BindingPolicy, PointIdentity, PointSpec},
    task::Tid,
};

const CLONE_OBSERVER_POINT: PointIdentity = PointIdentity::new(1);

// WIT consumer: `anemone:nemophila@0.1.0` / `weave-clone`.
//
// The canonical module-visible identity and value shape live in
// `nemophila/wit/nemophila.wit`. This owner-local adapter only projects task
// concepts into that shape; it does not own registration or runtime lifecycle.
const REGISTRATION_MODULE: &str = "anemone:nemophila/weave-clone@0.1.0";
const REGISTRATION_FUNCTION: &str = "register-observer";
const CALLBACK_EXPORT: &str = "observe-clone";

#[derive(Clone, Copy)]
pub(crate) struct CloneObservation {
    creator_tid: Tid,
    child_tid: Tid,
}

impl CloneObservation {
    pub(crate) const fn new(creator_tid: Tid, child_tid: Tid) -> Self {
        Self {
            creator_tid,
            child_tid,
        }
    }
}

pub(crate) struct CloneObserver;

impl PointSpec for CloneObserver {
    type Context = CloneObservation;
    type Params = (i32, i32);

    const ID: PointIdentity = CLONE_OBSERVER_POINT;
    const POLICY: BindingPolicy = BindingPolicy::Fanout;
    const REGISTRATION_MODULE: &'static str = REGISTRATION_MODULE;
    const REGISTRATION_FUNCTION: &'static str = REGISTRATION_FUNCTION;
    const CALLBACK_EXPORT: &'static str = CALLBACK_EXPORT;

    fn lower(context: &Self::Context) -> Self::Params {
        // Canonical ABI carries WIT `u32` values through Core Wasm i32 bit
        // patterns. Numeric signed conversion would corrupt high TIDs.
        (
            context.creator_tid.get() as i32,
            context.child_tid.get() as i32,
        )
    }
}

// Stage 4 ordinary builds intentionally contribute no production provider.
// KUnit proves that a real sibling subsystem can consume the declaration SPI;
// Stage 5 remains responsible for activating the task-owned production point.
#[cfg(feature = "kunit")]
crate::nemophila::weave::declare_provider!(
    pub(crate) static CLONE_OBSERVER: CloneObserver,
    __KUNIT_CLONE_PROVIDER
);
