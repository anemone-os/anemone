//! IO Multiplexing.

use crate::prelude::*;

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
    waiter: Option<&'a Arc<PollWaiter>>,
}

impl<'a> PollRequest<'a> {
    pub const fn snapshot(interests: PollEvent) -> Self {
        Self {
            interests,
            waiter: None,
        }
    }

    pub const fn interests(&self) -> PollEvent {
        self.interests
    }
}

// just draft.
#[derive(Debug)]
pub struct PollWaiter {
    event: Event,
    armed: AtomicBool,
}

impl PollWaiter {
    pub fn new() -> Self {
        Self {
            event: Event::new(),
            armed: AtomicBool::new(true),
        }
    }

    pub fn is_armed(&self) -> bool {
        self.armed.load(Ordering::Acquire)
    }

    pub fn disarm(&self) {
        self.armed.store(false, Ordering::Release);
    }

    pub fn wake(&self) {
        if self.armed.swap(false, Ordering::AcqRel) {
            self.event.publish(1, true);
        }
    }
}
