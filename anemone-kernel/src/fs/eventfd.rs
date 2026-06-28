//! Anonymous eventfd files.
//!
//! Eventfd readiness and blocking are owned entirely by the counter state here.
//! The anonymous inode only gives the opened file a stable fd identity.

use core::mem::size_of;

use crate::{
    fs::FileMode,
    prelude::*,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

const EVENTFD_MAX_COUNTER: u64 = u64::MAX - 1;

#[derive(Clone, Debug)]
struct EventFdPollTrigger {
    trigger: LatchTrigger,
    // Diagnostic only: readiness is recomputed under the eventfd state lock.
    interests: PollEvent,
}

impl EventFdPollTrigger {
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
struct EventFdIoTrigger {
    trigger: LatchTrigger,
}

impl EventFdIoTrigger {
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
struct EventFdDetachedTriggers {
    read: Vec<EventFdIoTrigger>,
    write: Vec<EventFdIoTrigger>,
    poll: Vec<EventFdPollTrigger>,
}

impl EventFdDetachedTriggers {
    fn empty() -> Self {
        Self {
            read: Vec::new(),
            write: Vec::new(),
            poll: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.read.is_empty() && self.write.is_empty() && self.poll.is_empty()
    }
}

#[derive(Debug)]
struct EventFdState {
    counter: u64,
    read_triggers: Vec<EventFdIoTrigger>,
    write_triggers: Vec<EventFdIoTrigger>,
    poll_triggers: Vec<EventFdPollTrigger>,
}

impl EventFdState {
    fn new(counter: u64) -> Self {
        Self {
            counter,
            read_triggers: Vec::new(),
            write_triggers: Vec::new(),
            poll_triggers: Vec::new(),
        }
    }

    fn revents(&self, interests: PollEvent) -> PollEvent {
        let mut revents = PollEvent::empty();
        if interests.contains(PollEvent::READABLE) && self.counter > 0 {
            revents |= PollEvent::READABLE;
        }
        if interests.contains(PollEvent::WRITABLE) && self.counter < EVENTFD_MAX_COUNTER {
            revents |= PollEvent::WRITABLE;
        }
        revents
    }

    fn register_read_wait(&mut self, trigger: &LatchTrigger) {
        self.prune_read_triggers();
        self.read_triggers.push(EventFdIoTrigger::new(trigger));
        kdebugln!(
            "eventfd: armed read wait={:#x} queue_len={}",
            trigger.wait_id(),
            self.read_triggers.len(),
        );
    }

    fn register_write_wait(&mut self, trigger: &LatchTrigger) {
        self.prune_write_triggers();
        self.write_triggers.push(EventFdIoTrigger::new(trigger));
        kdebugln!(
            "eventfd: armed write wait={:#x} queue_len={}",
            trigger.wait_id(),
            self.write_triggers.len(),
        );
    }

    fn register_poll_wait(&mut self, trigger: &LatchTrigger, interests: PollEvent) {
        self.prune_poll_triggers();
        self.poll_triggers
            .push(EventFdPollTrigger::new(trigger, interests));
        kdebugln!(
            "eventfd: armed poll wait={:#x} interests={:?} queue_len={}",
            trigger.wait_id(),
            interests,
            self.poll_triggers.len(),
        );
    }

    fn prune_read_triggers(&mut self) {
        self.read_triggers.retain(|trigger| !trigger.is_prunable());
    }

    fn prune_write_triggers(&mut self) {
        self.write_triggers.retain(|trigger| !trigger.is_prunable());
    }

    fn prune_poll_triggers(&mut self) {
        self.poll_triggers.retain(|trigger| !trigger.is_prunable());
    }

    fn detach_read_triggers(&mut self, reason: &'static str) -> Vec<EventFdIoTrigger> {
        self.prune_read_triggers();
        let detached = core::mem::take(&mut self.read_triggers);
        if !detached.is_empty() {
            kdebugln!(
                "eventfd: detached {} read triggers reason={}",
                detached.len(),
                reason,
            );
        }
        detached
    }

    fn detach_write_triggers(&mut self, reason: &'static str) -> Vec<EventFdIoTrigger> {
        self.prune_write_triggers();
        let detached = core::mem::take(&mut self.write_triggers);
        if !detached.is_empty() {
            kdebugln!(
                "eventfd: detached {} write triggers reason={}",
                detached.len(),
                reason,
            );
        }
        detached
    }

    fn detach_poll_triggers(
        &mut self,
        interests: PollEvent,
        reason: &'static str,
    ) -> Vec<EventFdPollTrigger> {
        self.prune_poll_triggers();
        let mut kept = Vec::new();
        let mut detached = Vec::new();

        for trigger in core::mem::take(&mut self.poll_triggers) {
            if trigger.interests.intersects(interests) {
                detached.push(trigger);
            } else {
                kept.push(trigger);
            }
        }
        self.poll_triggers = kept;

        if !detached.is_empty() {
            kdebugln!(
                "eventfd: detached {} poll triggers interests={:?} reason={}",
                detached.len(),
                interests,
                reason,
            );
        }
        detached
    }

    fn detach_readable_triggers(&mut self, reason: &'static str) -> EventFdDetachedTriggers {
        EventFdDetachedTriggers {
            read: self.detach_read_triggers(reason),
            write: Vec::new(),
            poll: self.detach_poll_triggers(PollEvent::READABLE, reason),
        }
    }

    fn detach_writable_triggers(&mut self, reason: &'static str) -> EventFdDetachedTriggers {
        EventFdDetachedTriggers {
            read: Vec::new(),
            write: self.detach_write_triggers(reason),
            poll: self.detach_poll_triggers(PollEvent::WRITABLE, reason),
        }
    }
}

#[derive(Debug, Opaque)]
struct EventFd {
    state: SpinLock<EventFdState>,
    semaphore: bool,
}

impl EventFd {
    fn new(counter: u64, semaphore: bool) -> Self {
        Self {
            state: SpinLock::new(EventFdState::new(counter)),
            semaphore,
        }
    }

    fn from_file(file: &File) -> &Self {
        file.prv()
            .cast::<EventFd>()
            .expect("eventfd file without eventfd private data")
    }

    fn wait_for_readable(&self) -> Result<(), SysError> {
        loop {
            if get_current_task().has_unmasked_signal() {
                return Err(SysError::Interrupted);
            }

            let latch = Latch::begin_current(true);
            let trigger = latch.make_trigger();

            {
                let mut state = self.state.lock();
                if state.counter > 0 {
                    latch.cancel(LatchCancelReason::PredicateReady);
                    let outcome = latch.finish();
                    kdebugln!(
                        "eventfd: read wait found readable before sleep outcome={:?}",
                        outcome,
                    );
                    return Ok(());
                }
                state.register_read_wait(&trigger);
            }

            latch.schedule_with_timeout(None);
            let outcome = latch.finish();
            match outcome {
                LatchWaitOutcome::Triggered => return Ok(()),
                LatchWaitOutcome::Signal | LatchWaitOutcome::Force => {
                    return Err(SysError::Interrupted);
                },
                LatchWaitOutcome::Cancelled | LatchWaitOutcome::Unexpected => {
                    kwarningln!("eventfd: unexpected read wait outcome={:?}", outcome);
                    return Err(SysError::IO);
                },
                LatchWaitOutcome::Timeout => {
                    kwarningln!("eventfd: blocking read wait timed out without timeout");
                    return Err(SysError::IO);
                },
            }
        }
    }

    fn wait_for_writable(&self, value: u64) -> Result<(), SysError> {
        loop {
            if get_current_task().has_unmasked_signal() {
                return Err(SysError::Interrupted);
            }

            let latch = Latch::begin_current(true);
            let trigger = latch.make_trigger();

            {
                let mut state = self.state.lock();
                if can_write_value(state.counter, value) {
                    latch.cancel(LatchCancelReason::PredicateReady);
                    let outcome = latch.finish();
                    kdebugln!(
                        "eventfd: write wait found writable before sleep outcome={:?}",
                        outcome,
                    );
                    return Ok(());
                }
                state.register_write_wait(&trigger);
            }

            latch.schedule_with_timeout(None);
            let outcome = latch.finish();
            match outcome {
                LatchWaitOutcome::Triggered => return Ok(()),
                LatchWaitOutcome::Signal | LatchWaitOutcome::Force => {
                    return Err(SysError::Interrupted);
                },
                LatchWaitOutcome::Cancelled | LatchWaitOutcome::Unexpected => {
                    kwarningln!("eventfd: unexpected write wait outcome={:?}", outcome);
                    return Err(SysError::IO);
                },
                LatchWaitOutcome::Timeout => {
                    kwarningln!("eventfd: blocking write wait timed out without timeout");
                    return Err(SysError::IO);
                },
            }
        }
    }
}

fn can_write_value(counter: u64, value: u64) -> bool {
    u64::MAX - counter > value
}

fn trigger_detached_triggers(triggers: EventFdDetachedTriggers, reason: &'static str) {
    if triggers.is_empty() {
        return;
    }

    for trigger in triggers.read {
        kdebugln!(
            "eventfd: trigger read wait={:#x} reason={}",
            trigger.trigger.wait_id(),
            reason,
        );
        trigger.trigger.trigger();
    }

    for trigger in triggers.write {
        kdebugln!(
            "eventfd: trigger write wait={:#x} reason={}",
            trigger.trigger.wait_id(),
            reason,
        );
        trigger.trigger.trigger();
    }

    for trigger in triggers.poll {
        kdebugln!(
            "eventfd: trigger poll wait={:#x} interests={:?} reason={}",
            trigger.trigger.wait_id(),
            trigger.interests,
            reason,
        );
        trigger.trigger.trigger();
    }
}

fn eventfd_read(
    file: &File,
    _pos: &mut usize,
    buf: &mut [u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    if buf.len() < size_of::<u64>() {
        return Err(SysError::InvalidArgument);
    }

    let eventfd = EventFd::from_file(file);
    loop {
        let result = {
            let mut state = eventfd.state.lock();
            if state.counter == 0 {
                None
            } else {
                let value = if eventfd.semaphore {
                    state.counter -= 1;
                    1
                } else {
                    let value = state.counter;
                    state.counter = 0;
                    value
                };
                let detached = state.detach_writable_triggers("read");
                Some((value, detached))
            }
        };

        if let Some((value, detached)) = result {
            buf[..size_of::<u64>()].copy_from_slice(&value.to_le_bytes());
            trigger_detached_triggers(detached, "read");
            return Ok(size_of::<u64>());
        }

        if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
            return Err(SysError::Again);
        }
        eventfd.wait_for_readable()?;
    }
}

fn eventfd_write(
    file: &File,
    _pos: &mut usize,
    buf: &[u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    if buf.len() < size_of::<u64>() {
        return Err(SysError::InvalidArgument);
    }

    let value = u64::from_le_bytes(
        buf[..size_of::<u64>()]
            .try_into()
            .expect("eventfd write value slice has exact u64 length"),
    );
    if value == u64::MAX {
        return Err(SysError::InvalidArgument);
    }

    let eventfd = EventFd::from_file(file);
    loop {
        let result = {
            let mut state = eventfd.state.lock();
            if can_write_value(state.counter, value) {
                let was_zero = state.counter == 0;
                state.counter = state
                    .counter
                    .checked_add(value)
                    .expect("eventfd checked write must not overflow");
                let detached = if was_zero && value > 0 {
                    state.detach_readable_triggers("write")
                } else {
                    EventFdDetachedTriggers::empty()
                };
                Some(detached)
            } else {
                None
            }
        };

        if let Some(detached) = result {
            trigger_detached_triggers(detached, "write");
            return Ok(size_of::<u64>());
        }

        if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
            return Err(SysError::Again);
        }
        eventfd.wait_for_writable(value)?;
    }
}

fn eventfd_poll(file: &File, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
    let eventfd = EventFd::from_file(file);
    let mut state = eventfd.state.lock();
    let revents = state.revents(request.interests());
    if !revents.is_empty() || !request.is_register() {
        return Ok(PollRegisterResult::Ready(revents));
    }

    let armable = request
        .interests()
        .intersects(PollEvent::READABLE | PollEvent::WRITABLE);
    if !armable {
        return Ok(PollRegisterResult::Unsupported);
    }

    let trigger = request
        .trigger()
        .expect("register request disappeared after is_register");
    state.register_poll_wait(trigger, request.interests());
    Ok(PollRegisterResult::Armed)
}

fn eventfd_check_status_flags(_file: &File, flags: FileOpStatusFlags) -> Result<(), SysError> {
    if flags.contains(FileOpStatusFlags::DIRECT) {
        knoticeln!("eventfd: rejecting O_DIRECT status flag");
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

static EVENTFD_FILE_OPS: FileOps = FileOps {
    read: eventfd_read,
    write: eventfd_write,
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    check_status_flags: eventfd_check_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: eventfd_poll,
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

fn eventfd_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let meta = inode.inode().meta_snapshot();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: meta.nlink,
        uid: meta.uid,
        gid: meta.gid,
        rdev: DeviceId::None,
        size: meta.size,
        atime: meta.atime,
        mtime: meta.mtime,
        ctime: meta.ctime,
    })
}

static EVENTFD_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| unreachable!("eventfd files are opened with explicit private state"),
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: eventfd_get_attr,
};

pub fn create_eventfd(counter: u32, semaphore: bool) -> Result<File, SysError> {
    let path = anony_new_inode(InodeType::Regular, &EVENTFD_INODE_OPS, NilOpaque::new())?;
    anony_open_with(
        &path,
        OpenedFile::with_mode(
            &EVENTFD_FILE_OPS,
            FileMode::STREAM,
            AnyOpaque::new(EventFd::new(counter as u64, semaphore)),
        ),
    )
}
