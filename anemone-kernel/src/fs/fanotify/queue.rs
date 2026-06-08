use crate::prelude::*;

use super::event::FanEvent;

pub const DEFAULT_MAX_EVENTS: usize = 16_384;

#[derive(Clone, Debug)]
pub(super) struct FanPollTrigger {
    trigger: LatchTrigger,
    interests: PollEvent,
}

impl FanPollTrigger {
    fn new(trigger: &LatchTrigger, interests: PollEvent) -> Self {
        Self {
            trigger: trigger.clone(),
            interests,
        }
    }

    fn is_prunable(&self) -> bool {
        self.trigger.is_prunable()
    }
}

#[derive(Clone, Debug)]
pub(super) struct FanReadTrigger {
    trigger: LatchTrigger,
}

impl FanReadTrigger {
    fn new(trigger: &LatchTrigger) -> Self {
        Self {
            trigger: trigger.clone(),
        }
    }

    fn is_prunable(&self) -> bool {
        self.trigger.is_prunable()
    }
}

#[derive(Debug)]
pub(super) struct FanDetachedTriggers {
    poll: Vec<FanPollTrigger>,
    read: Vec<FanReadTrigger>,
}

impl FanDetachedTriggers {
    fn empty() -> Self {
        Self {
            poll: Vec::new(),
            read: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct FanQueue {
    events: VecDeque<FanEvent>,
    max_events: usize,
    overflow_queued: bool,
    dropped_events: u64,
    poll_triggers: Vec<FanPollTrigger>,
    read_triggers: Vec<FanReadTrigger>,
}

impl FanQueue {
    pub fn new(max_events: usize) -> Self {
        assert!(max_events > 0, "fanotify queue cap must be non-zero");
        Self {
            events: VecDeque::new(),
            max_events,
            overflow_queued: false,
            dropped_events: 0,
            poll_triggers: Vec::new(),
            read_triggers: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn queued_bytes(&self) -> usize {
        self.events
            .iter()
            .map(FanEvent::metadata_len)
            .fold(0usize, |acc, len| acc.saturating_add(len))
    }

    pub fn pop_front(&mut self) -> Option<FanEvent> {
        let event = self.events.pop_front()?;
        if event.mask().contains(super::types::FanMask::Q_OVERFLOW) {
            self.overflow_queued = self
                .events
                .iter()
                .any(|event| event.mask().contains(super::types::FanMask::Q_OVERFLOW));
        }
        Some(event)
    }

    pub fn enqueue(&mut self, event: FanEvent) -> FanDetachedTriggers {
        let was_empty = self.events.is_empty();
        if self.events.len() < self.max_events {
            self.events.push_back(event);
        } else if !self.overflow_queued {
            // Keep the queue bounded while still publishing one observable
            // overflow sentinel. The dropped tail is intentionally not merged:
            // precise Linux merge/order semantics are deferred by the RFC, but
            // an unbounded or silent queue is not allowed before VFS enqueue.
            let _ = self.events.pop_back();
            self.events.push_back(FanEvent::overflow());
            self.overflow_queued = true;
            self.dropped_events = self.dropped_events.saturating_add(1);
        } else {
            self.dropped_events = self.dropped_events.saturating_add(1);
        }

        if was_empty && !self.events.is_empty() {
            self.detach_wait_triggers("enqueue")
        } else {
            FanDetachedTriggers::empty()
        }
    }

    pub fn clear(&mut self) -> FanDetachedTriggers {
        self.events.clear();
        self.overflow_queued = false;
        self.detach_wait_triggers("clear")
    }

    pub fn poll(&mut self, request: &PollRequest<'_>, dead: bool) -> PollRegisterResult {
        let mut revents = PollEvent::empty();
        if request.interests().contains(PollEvent::READABLE) && !self.events.is_empty() {
            revents |= PollEvent::READABLE;
        }
        if dead {
            revents |= PollEvent::HANG_UP;
        }

        if !revents.is_empty() || !request.is_register() {
            return PollRegisterResult::Ready(revents);
        }

        let trigger = request
            .trigger()
            .expect("register request disappeared after is_register");
        self.prune_poll_triggers();
        self.poll_triggers
            .push(FanPollTrigger::new(trigger, request.interests()));

        kdebugln!(
            "fanotify: armed poll wait={:#x} interests={:?} queue_len={}",
            trigger.wait_id(),
            request.interests(),
            self.poll_triggers.len(),
        );

        PollRegisterResult::Armed
    }

    pub fn register_read_wait(&mut self, trigger: &LatchTrigger) {
        self.prune_read_triggers();
        self.read_triggers.push(FanReadTrigger::new(trigger));

        kdebugln!(
            "fanotify: armed read wait={:#x} queue_len={}",
            trigger.wait_id(),
            self.read_triggers.len(),
        );
    }

    fn prune_poll_triggers(&mut self) {
        self.poll_triggers.retain(|trigger| !trigger.is_prunable());
    }

    fn prune_read_triggers(&mut self) {
        self.read_triggers.retain(|trigger| !trigger.is_prunable());
    }

    fn detach_poll_triggers(&mut self, reason: &'static str) -> Vec<FanPollTrigger> {
        self.prune_poll_triggers();
        let detached = core::mem::take(&mut self.poll_triggers);
        if !detached.is_empty() {
            kdebugln!(
                "fanotify: detached {} poll triggers reason={}",
                detached.len(),
                reason,
            );
        }
        detached
    }

    fn detach_read_triggers(&mut self, reason: &'static str) -> Vec<FanReadTrigger> {
        self.prune_read_triggers();
        let detached = core::mem::take(&mut self.read_triggers);
        if !detached.is_empty() {
            kdebugln!(
                "fanotify: detached {} read triggers reason={}",
                detached.len(),
                reason,
            );
        }
        detached
    }

    fn detach_wait_triggers(&mut self, reason: &'static str) -> FanDetachedTriggers {
        FanDetachedTriggers {
            poll: self.detach_poll_triggers(reason),
            read: self.detach_read_triggers(reason),
        }
    }
}

pub(super) fn trigger_detached_triggers(
    triggers: FanDetachedTriggers,
    reason: &'static str,
) {
    for trigger in triggers.poll {
        kdebugln!(
            "fanotify: trigger poll wait={:#x} interests={:?} reason={}",
            trigger.trigger.wait_id(),
            trigger.interests,
            reason,
        );
        trigger.trigger.trigger();
    }

    for trigger in triggers.read {
        kdebugln!(
            "fanotify: trigger read wait={:#x} reason={}",
            trigger.trigger.wait_id(),
            reason,
        );
        trigger.trigger.trigger();
    }
}
