use crate::prelude::*;

use super::{
    event::FanEvent,
    mark::MarkHandle,
    queue::{DEFAULT_MAX_EVENTS, FanDetachedTriggers, FanQueue, trigger_detached_triggers},
    registry,
    types::{FanEventFdTemplate, FanGroupMode},
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
    mark_handles: Vec<MarkHandle>,
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
    generation: u64,
    mode: FanGroupMode,
    event_fd_template: FanEventFdTemplate,
    state: Mutex<FanGroupState>,
}

impl FanGroup {
    pub fn new(mode: FanGroupMode, event_fd_template: FanEventFdTemplate) -> Arc<Self> {
        Arc::new(Self {
            id: FanGroupId::next(),
            generation: 1,
            mode,
            event_fd_template,
            state: Mutex::new(FanGroupState {
                queue: FanQueue::new(DEFAULT_MAX_EVENTS),
                mark_handles: Vec::new(),
                dead: false,
            }),
        })
    }

    pub const fn id(&self) -> FanGroupId {
        self.id
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn mode(&self) -> FanGroupMode {
        self.mode
    }

    pub const fn event_fd_template(&self) -> FanEventFdTemplate {
        self.event_fd_template
    }

    pub fn is_dead(&self) -> bool {
        self.state.lock().dead
    }

    pub(super) fn add_mark_handle_from_registry(&self, handle: MarkHandle) -> Result<(), SysError> {
        let mut state = self.state.lock();
        if state.dead {
            return Err(SysError::InvalidArgument);
        }
        assert!(
            state
                .mark_handles
                .iter()
                .all(|existing| *existing != handle),
            "fanotify group cleanup list cannot contain duplicate mark handles"
        );
        state.mark_handles.push(handle);
        Ok(())
    }

    pub(super) fn remove_mark_handle_from_registry(&self, handle: MarkHandle) {
        let mut state = self.state.lock();
        state.mark_handles.retain(|existing| *existing != handle);
    }

    pub(super) fn begin_mark_dead_from_registry(&self) -> Option<Vec<MarkHandle>> {
        let mut state = self.state.lock();
        if state.dead {
            return None;
        }
        state.dead = true;
        Some(core::mem::take(&mut state.mark_handles))
    }

    pub(super) fn clear_dead_queue_after_registry(&self) -> FanDetachedTriggers {
        let mut state = self.state.lock();
        assert!(
            state.dead,
            "fanotify dead-queue cleanup must follow registry-owned death mark"
        );
        state.queue.clear()
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
        if let Some(detached) = registry::mark_group_dead(self) {
            trigger_detached_triggers(detached, "group_dead");
        }
    }

    pub fn wait_for_event(&self) -> Result<Option<FanEvent>, SysError> {
        loop {
            {
                let mut state = self.state.lock();
                if state.dead {
                    return Ok(None);
                }
                if let Some(event) = state.queue.pop_front() {
                    return Ok(Some(event));
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
                if state.dead {
                    latch.cancel(LatchCancelReason::PredicateReady);
                    let outcome = latch.finish();
                    kdebugln!(
                        "fanotify: read wait found dead group before sleep outcome={:?}",
                        outcome,
                    );
                    return Ok(None);
                }
                if let Some(event) = state.queue.pop_front() {
                    latch.cancel(LatchCancelReason::PredicateReady);
                    let outcome = latch.finish();
                    kdebugln!(
                        "fanotify: read wait found queued event before sleep outcome={:?}",
                        outcome,
                    );
                    return Ok(Some(event));
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
