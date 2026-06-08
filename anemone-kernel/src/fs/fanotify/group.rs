use crate::prelude::*;

use super::{
    event::FanEvent,
    queue::{DEFAULT_MAX_EVENTS, FanQueue, trigger_detached_triggers},
    types::{FanEventFdTemplate, FanGroupMode, FanInitFlags},
};

static NEXT_GROUP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FanGroupId(u64);

impl FanGroupId {
    fn next() -> Self {
        Self(NEXT_GROUP_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug)]
struct FanGroupState {
    queue: FanQueue,
    dead: bool,
}

#[derive(Debug)]
pub(super) enum FanReadState {
    Event(FanEvent),
    Empty,
    Dead,
}

#[derive(Debug)]
pub struct FanGroup {
    id: FanGroupId,
    mode: FanGroupMode,
    init_flags: FanInitFlags,
    event_fd_template: FanEventFdTemplate,
    state: Mutex<FanGroupState>,
}

impl FanGroup {
    pub fn new(
        mode: FanGroupMode,
        init_flags: FanInitFlags,
        event_fd_template: FanEventFdTemplate,
    ) -> Arc<Self> {
        Arc::new(Self {
            id: FanGroupId::next(),
            mode,
            init_flags,
            event_fd_template,
            state: Mutex::new(FanGroupState {
                queue: FanQueue::new(DEFAULT_MAX_EVENTS),
                dead: false,
            }),
        })
    }

    pub const fn id(&self) -> FanGroupId {
        self.id
    }

    pub const fn mode(&self) -> FanGroupMode {
        self.mode
    }

    pub const fn init_flags(&self) -> FanInitFlags {
        self.init_flags
    }

    pub const fn event_fd_template(&self) -> FanEventFdTemplate {
        self.event_fd_template
    }

    pub fn enqueue(&self, event: FanEvent) {
        let detached = {
            let mut state = self.state.lock();
            if state.dead {
                return;
            }
            state.queue.enqueue(event)
        };
        trigger_detached_triggers(detached, "enqueue");
    }

    pub(super) fn pop_read_state(&self) -> Result<FanReadState, SysError> {
        let mut state = self.state.lock();
        if state.dead {
            return Ok(FanReadState::Dead);
        }
        match state.queue.pop_front() {
            Some(event) => Ok(FanReadState::Event(event)),
            None => Ok(FanReadState::Empty),
        }
    }

    pub fn queued_bytes(&self) -> usize {
        self.state.lock().queue.queued_bytes()
    }

    pub fn poll(&self, request: &PollRequest<'_>) -> PollRegisterResult {
        let mut state = self.state.lock();
        let dead = state.dead;
        state.queue.poll(request, dead)
    }

    pub fn mark_dead(&self) {
        let detached = {
            let mut state = self.state.lock();
            if state.dead {
                return;
            }
            state.dead = true;
            state.queue.clear()
        };
        trigger_detached_triggers(detached, "group_dead");
    }

    pub fn wait_for_event(&self) -> Result<Option<FanEvent>, SysError> {
        loop {
            {
                let mut state = self.state.lock();
                if let Some(event) = state.queue.pop_front() {
                    return Ok(Some(event));
                }
                if state.dead {
                    return Ok(None);
                }
            }

            if get_current_task().has_unmasked_signal() {
                return Err(SysError::Interrupted);
            }

            let latch = Latch::begin_current(true);
            let trigger = latch.make_trigger();

            // The read waiter is published and the queue/dead predicates are
            // rechecked while holding the group lock. Enqueue and group death
            // detach triggers under the same lock and fire them after unlock,
            // so no event/dead transition can be lost between the empty check
            // and the blocking schedule.
            {
                let mut state = self.state.lock();
                if let Some(event) = state.queue.pop_front() {
                    latch.cancel(LatchCancelReason::PredicateReady);
                    let outcome = latch.finish();
                    kdebugln!(
                        "fanotify: read wait found queued event before sleep outcome={:?}",
                        outcome,
                    );
                    return Ok(Some(event));
                }
                if state.dead {
                    latch.cancel(LatchCancelReason::PredicateReady);
                    let outcome = latch.finish();
                    kdebugln!(
                        "fanotify: read wait found dead group before sleep outcome={:?}",
                        outcome,
                    );
                    return Ok(None);
                }
                state.queue.register_read_wait(&trigger);
            }

            latch.schedule_with_timeout(None);
            let outcome = latch.finish();
            match outcome {
                LatchWaitOutcome::Triggered => {},
                LatchWaitOutcome::Signal | LatchWaitOutcome::Force => {
                    return Err(SysError::Interrupted);
                },
                LatchWaitOutcome::Cancelled | LatchWaitOutcome::Unexpected => {
                    kwarningln!(
                        "fanotify: unexpected blocking read wait outcome={:?}",
                        outcome,
                    );
                    return Err(SysError::IO);
                },
                LatchWaitOutcome::Timeout => {
                    kwarningln!("fanotify: blocking read wait timed out without timeout");
                    return Err(SysError::IO);
                },
            }
        }
    }
}
