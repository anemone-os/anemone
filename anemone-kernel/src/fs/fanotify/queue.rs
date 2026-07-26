use crate::{fs::iomux::PollRoute, prelude::*};

use super::event::FanEvent;

pub const DEFAULT_MAX_EVENTS: usize = 16_384;

#[derive(Clone, Debug)]
pub(super) struct FanPollRoute {
    route: PollRoute,
    interests: PollEvent,
}

impl FanPollRoute {
    fn new(route: &PollRoute, interests: PollEvent) -> Self {
        Self {
            route: route.clone(),
            interests,
        }
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
    poll_routes: Option<Arc<Vec<FanPollRoute>>>,
    changed: PollEvent,
    read: Vec<FanReadTrigger>,
}

impl FanDetachedTriggers {
    fn empty() -> Self {
        Self {
            poll_routes: None,
            changed: PollEvent::empty(),
            read: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct FanQueue {
    events: VecDeque<FanEvent>,
    max_events: usize,
    overflow_queued: bool,
    // Telemetry only until a later resource-limit/fdinfo gate deliberately
    // exposes overflow accounting. Queue behavior is driven by overflow_queued.
    dropped_events: u64,
    /// Subscription builds a replacement before publication. Queue/dead
    /// transitions clone this snapshot while locked; route notification and
    /// old-snapshot drop happen after the group mutex is released.
    poll_routes: Arc<Vec<FanPollRoute>>,
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
            poll_routes: Arc::new(Vec::new()),
            read_triggers: Vec::new(),
        }
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
            self.collect_waiters(PollEvent::READABLE, "enqueue")
        } else {
            FanDetachedTriggers::empty()
        }
    }

    pub fn clear(&mut self) -> FanDetachedTriggers {
        self.events.clear();
        self.overflow_queued = false;
        self.collect_waiters(PollEvent::HANG_UP, "clear")
    }

    pub fn poll(
        &mut self,
        request: &PollRequest<'_>,
        dead: bool,
    ) -> Result<(PollRegisterResult, Option<Arc<Vec<FanPollRoute>>>), SysError> {
        if !request.is_register() {
            return Ok((
                PollRegisterResult::Ready(self.revents(request.interests(), dead)),
                None,
            ));
        }

        let route = request
            .route()
            .expect("register request disappeared after is_register");
        let (previous_routes, pruned) =
            replace_fan_poll_routes(&mut self.poll_routes, route, request.interests())?;
        let revents = self.revents(request.interests(), dead);
        let queue_len = self.poll_routes.len();

        kdebugln!(
            "fanotify: subscribed poll interests={:?} queue_len={} pruned={}",
            request.interests(),
            queue_len,
            pruned,
        );

        Ok((
            PollRegisterResult::Subscribed(revents),
            Some(previous_routes),
        ))
    }

    fn revents(&self, interests: PollEvent, dead: bool) -> PollEvent {
        let mut revents = PollEvent::empty();
        if interests.contains(PollEvent::READABLE) && !self.events.is_empty() {
            revents |= PollEvent::READABLE;
        }
        if dead {
            revents |= PollEvent::HANG_UP;
        }
        revents
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

    fn prune_read_triggers(&mut self) {
        self.read_triggers.retain(|trigger| !trigger.is_prunable());
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

    fn collect_waiters(&mut self, changed: PollEvent, reason: &'static str) -> FanDetachedTriggers {
        FanDetachedTriggers {
            poll_routes: Some(self.poll_routes.clone()),
            changed,
            read: self.detach_read_triggers(reason),
        }
    }
}

fn replace_fan_poll_routes(
    routes: &mut Arc<Vec<FanPollRoute>>,
    route: &PollRoute,
    interests: PollEvent,
) -> Result<(Arc<Vec<FanPollRoute>>, usize), SysError> {
    let retained = routes
        .iter()
        .filter(|entry| !entry.route.is_prunable())
        .count();
    let capacity = retained.checked_add(1).ok_or(SysError::OutOfMemory)?;
    let mut replacement = Vec::new();
    replacement
        .try_reserve(capacity)
        .map_err(|_| SysError::OutOfMemory)?;
    replacement.extend(
        routes
            .iter()
            .filter(|entry| !entry.route.is_prunable())
            .cloned(),
    );
    let pruned = routes.len() - replacement.len();
    replacement.push(FanPollRoute::new(route, interests));

    let replacement = Arc::try_new(replacement).map_err(|_| SysError::OutOfMemory)?;
    Ok((core::mem::replace(routes, replacement), pruned))
}

pub(super) fn trigger_detached_triggers(triggers: FanDetachedTriggers, reason: &'static str) {
    if let Some(routes) = triggers.poll_routes {
        for entry in routes.iter() {
            if triggers.changed.contains(PollEvent::HANG_UP)
                || entry.interests.intersects(triggers.changed)
            {
                entry.route.notify();
            }
        }
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
