//! IO Multiplexing.

use crate::prelude::*;

mod subscription;
mod wait;

pub(in crate::fs) use subscription::PollObserver;
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
}

impl<'a> PollRequest<'a> {
    pub const fn snapshot(interests: PollEvent) -> Self {
        Self {
            interests,
            route: None,
        }
    }

    pub(in crate::fs) const fn register_with_route(
        interests: PollEvent,
        route: &'a PollRoute,
    ) -> Self {
        Self {
            interests,
            route: Some(route),
        }
    }

    pub const fn interests(&self) -> PollEvent {
        self.interests
    }

    pub(crate) const fn route(&self) -> Option<&'a PollRoute> {
        self.route
    }

    pub const fn is_register(&self) -> bool {
        self.route.is_some()
    }

    /// Convert a source-local readiness snapshot into the typed poll result.
    ///
    /// Snapshot requests may return an empty ready set. Register requests must
    /// fail closed when the source cannot publish a route for a currently
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
