//! IO Multiplexing.

use crate::prelude::*;

mod subscription;
mod wait;

pub(crate) use subscription::PollRoute;
pub(crate) use wait::IomuxWaitRound;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PollEvent: u32 {
        const READABLE = 0x01;
        const WRITABLE = 0x02;
        const ERROR = 0x04;
        const HANG_UP = 0x08;
        // TODO
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PollRequest<'a> {
    interests: PollEvent,
    route: Option<&'a PollRoute>,
    /// Stage 0 compatibility bridge for sources not yet migrated to routes.
    ///
    /// New source code must consume `route`; this field and the two-argument
    /// register constructor disappear at Stage 1 `SUBSCRIPTION-CUTOVER`.
    trigger: Option<&'a LatchTrigger>,
}

impl<'a> PollRequest<'a> {
    pub const fn snapshot(interests: PollEvent) -> Self {
        Self {
            interests,
            route: None,
            trigger: None,
        }
    }

    /// Legacy register request retained only until `SUBSCRIPTION-CUTOVER`.
    pub const fn register(interests: PollEvent, trigger: &'a LatchTrigger) -> Self {
        Self {
            interests,
            route: None,
            trigger: Some(trigger),
        }
    }

    pub(in crate::fs) const fn register_with_route(
        interests: PollEvent,
        route: &'a PollRoute,
        trigger: &'a LatchTrigger,
    ) -> Self {
        Self {
            interests,
            route: Some(route),
            trigger: Some(trigger),
        }
    }

    pub const fn interests(&self) -> PollEvent {
        self.interests
    }

    pub const fn trigger(&self) -> Option<&'a LatchTrigger> {
        self.trigger
    }

    pub(crate) const fn route(&self) -> Option<&'a PollRoute> {
        self.route
    }

    pub const fn is_register(&self) -> bool {
        self.trigger.is_some()
    }

    /// Convert a source-local readiness snapshot into the typed poll result.
    ///
    /// Snapshot requests may return an empty ready set. Register requests must
    /// fail closed when the source cannot arm a trigger for a currently
    /// not-ready predicate.
    pub fn ready_or_unsupported(&self, events: PollEvent) -> PollRegisterResult {
        if self.is_register() && events.is_empty() {
            PollRegisterResult::Unsupported
        } else {
            PollRegisterResult::Ready(events)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollRegisterResult {
    Ready(PollEvent),
    /// A persistent route was installed before this readiness snapshot.
    Subscribed(PollEvent),
    Armed,
    Unsupported,
}

impl PollRegisterResult {
    pub const fn ready(events: PollEvent) -> Self {
        Self::Ready(events)
    }

    pub fn expect_ready(self, context: &str) -> PollEvent {
        match self {
            Self::Ready(events) => events,
            other => {
                kwarningln!(
                    "iomux: {} expected snapshot ready result, got {:?}",
                    context,
                    other,
                );
                assert!(false, "poll snapshot returned non-ready typed result");
                PollEvent::empty()
            },
        }
    }
}
