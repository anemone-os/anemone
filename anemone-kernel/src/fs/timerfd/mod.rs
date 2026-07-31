//! Anonymous timerfd files.
//!
//! Timerfd readiness is owned by the private timerfd state. The anonymous inode
//! only provides a stable fd identity; timer expiration, blocking reads, poll
//! readiness, and logical cancellation all live in `TimerFdCore`.

mod api;

use core::mem::size_of;

use anemone_abi::time::linux::{ITimerSpec, TimeSpec};

use crate::{
    fs::FileMode,
    prelude::*,
    time::{clock::get_clock, timer::schedule_threaded_timer_event},
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::iomux::PollRoute;

const NSEC_PER_SEC: u64 = 1_000_000_000;
const TIMERFD_TRIGGER_QUEUE_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TimerFdSettimeFlags {
    abstime: bool,
    cancel_on_set: bool,
}

#[derive(Clone, Debug)]
struct TimerFdPollRoute {
    route: PollRoute,
    // Diagnostic only: readiness is recomputed under the timerfd state lock.
    interests: PollEvent,
}

impl TimerFdPollRoute {
    fn new(route: &PollRoute, interests: PollEvent) -> Self {
        Self {
            route: route.clone(),
            interests,
        }
    }

    fn is_prunable(&self) -> bool {
        self.route.is_prunable()
    }
}

#[derive(Clone, Debug)]
struct TimerFdIoTrigger {
    trigger: LatchTrigger,
}

impl TimerFdIoTrigger {
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
struct TimerFdHandoffBatch {
    // Caller-owned handoff built while holding TimerFdState's no-IRQ lock.
    // Read triggers and stale routes are removed; live poll routes are cloned
    // for notification. Every notify and drop is consumed after unlock.
    read: heapless::Vec<TimerFdIoTrigger, TIMERFD_TRIGGER_QUEUE_CAPACITY>,
    poll_notify: heapless::Vec<TimerFdPollRoute, TIMERFD_TRIGGER_QUEUE_CAPACITY>,
    poll_stale: heapless::Vec<TimerFdPollRoute, TIMERFD_TRIGGER_QUEUE_CAPACITY>,
}

impl TimerFdHandoffBatch {
    fn empty() -> Self {
        Self {
            read: heapless::Vec::new(),
            poll_notify: heapless::Vec::new(),
            poll_stale: heapless::Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.read.is_empty() && self.poll_notify.is_empty() && self.poll_stale.is_empty()
    }

    fn push_read(&mut self, trigger: TimerFdIoTrigger) {
        assert!(
            self.read.push(trigger).is_ok(),
            "timerfd read trigger batch overflow"
        );
    }

    fn push_poll_notify(&mut self, route: TimerFdPollRoute) {
        assert!(
            self.poll_notify.push(route).is_ok(),
            "timerfd poll notify batch overflow"
        );
    }

    fn push_poll_stale(&mut self, route: TimerFdPollRoute) {
        assert!(
            self.poll_stale.push(route).is_ok(),
            "timerfd stale poll route batch overflow"
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerFdSchedule {
    Disarmed,
    Armed {
        next_expire_at_ns: u64,
        interval_ns: Option<u64>,
    },
}

#[derive(Debug)]
struct TimerFdState {
    generation: u64,
    schedule: TimerFdSchedule,
    expirations: u64,
    read_triggers: Vec<TimerFdIoTrigger>,
    poll_routes: Vec<TimerFdPollRoute>,
    // Diagnostic only: accepted no-op state for the stage-1
    // TFD_TIMER_CANCEL_ON_SET compatibility bridge. It must not drive read
    // errors until clock-set cancellation is implemented.
    cancel_on_set_accepted: bool,
}

impl TimerFdState {
    fn new() -> Result<Self, SysError> {
        Ok(Self {
            generation: 0,
            schedule: TimerFdSchedule::Disarmed,
            expirations: 0,
            read_triggers: queue_with_capacity()?,
            poll_routes: queue_with_capacity()?,
            cancel_on_set_accepted: false,
        })
    }

    fn revents(&self, interests: PollEvent) -> PollEvent {
        if interests.contains(PollEvent::READABLE) && self.expirations > 0 {
            PollEvent::READABLE
        } else {
            PollEvent::empty()
        }
    }

    fn register_read_wait(
        &mut self,
        trigger: &LatchTrigger,
        stale: &mut TimerFdHandoffBatch,
    ) -> Result<(), SysError> {
        self.detach_prunable_read_triggers(stale);
        if self.read_triggers.len() >= TIMERFD_TRIGGER_QUEUE_CAPACITY {
            return Err(SysError::OutOfMemory);
        }
        self.read_triggers.push(TimerFdIoTrigger::new(trigger));
        Ok(())
    }

    fn register_poll_route(
        &mut self,
        route: &PollRoute,
        interests: PollEvent,
        stale: &mut TimerFdHandoffBatch,
    ) -> bool {
        self.detach_prunable_poll_routes(stale);
        if self.poll_routes.len() >= TIMERFD_TRIGGER_QUEUE_CAPACITY {
            return false;
        }
        self.poll_routes
            .push(TimerFdPollRoute::new(route, interests));
        true
    }

    fn detach_prunable_read_triggers(&mut self, stale: &mut TimerFdHandoffBatch) {
        let mut index = 0;
        while index < self.read_triggers.len() {
            if self.read_triggers[index].is_prunable() {
                stale.push_read(self.read_triggers.swap_remove(index));
            } else {
                index += 1;
            }
        }
    }

    fn detach_prunable_poll_routes(&mut self, stale: &mut TimerFdHandoffBatch) {
        let mut index = 0;
        while index < self.poll_routes.len() {
            if self.poll_routes[index].is_prunable() {
                stale.push_poll_stale(self.poll_routes.swap_remove(index));
            } else {
                index += 1;
            }
        }
    }

    fn collect_readable_waiters(&mut self) -> TimerFdHandoffBatch {
        // Blocking-read triggers remain one-shot and are detached. Poll routes
        // are persistent: clone live routes into the caller-owned notify batch,
        // retain them in the registry, and move only stale routes out. All
        // notification and drop then happens after the no-IRQ guard is released.
        let mut handoff = TimerFdHandoffBatch::empty();
        while let Some(trigger) = self.read_triggers.pop() {
            handoff.push_read(trigger);
        }
        let mut index = 0;
        while index < self.poll_routes.len() {
            if self.poll_routes[index].is_prunable() {
                handoff.push_poll_stale(self.poll_routes.swap_remove(index));
            } else {
                handoff.push_poll_notify(self.poll_routes[index].clone());
                index += 1;
            }
        }
        handoff
    }
}

#[derive(Debug)]
struct TimerFdCore {
    state: NoIrqSpinLock<TimerFdState>,
    clockid: i32,
}

impl TimerFdCore {
    fn new(clockid: i32) -> Result<Self, SysError> {
        Ok(Self {
            state: NoIrqSpinLock::new(TimerFdState::new()?),
            clockid,
        })
    }

    fn now_ns(&self) -> u64 {
        get_clock(self.clockid as usize)
            .expect("validated timerfd clock disappeared")
            .now_ns()
    }
}

#[derive(Debug, Opaque)]
struct TimerFdFile {
    core: Arc<TimerFdCore>,
}

impl TimerFdFile {
    fn new(clockid: i32) -> Result<Self, SysError> {
        Ok(Self {
            core: Arc::new(TimerFdCore::new(clockid)?),
        })
    }

    fn from_file(file: &File) -> Option<&Self> {
        file.prv().cast::<TimerFdFile>()
    }

    fn core_from_file(file: &File) -> Result<Arc<TimerFdCore>, SysError> {
        Self::from_file(file)
            .map(|timerfd| timerfd.core.clone())
            .ok_or(SysError::InvalidArgument)
    }
}

fn queue_with_capacity<T>() -> Result<Vec<T>, SysError> {
    let mut queue = Vec::new();
    queue
        .try_reserve_exact(TIMERFD_TRIGGER_QUEUE_CAPACITY)
        .map_err(|_| SysError::OutOfMemory)?;
    Ok(queue)
}

fn timespec_to_ns(ts: TimeSpec) -> Result<u64, SysError> {
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= NSEC_PER_SEC as i64 {
        return Err(SysError::InvalidArgument);
    }
    let sec_ns = (ts.tv_sec as u64)
        .checked_mul(NSEC_PER_SEC)
        .ok_or(SysError::InvalidArgument)?;
    sec_ns
        .checked_add(ts.tv_nsec as u64)
        .ok_or(SysError::InvalidArgument)
}

fn ns_to_timespec(ns: u64) -> TimeSpec {
    TimeSpec {
        tv_sec: (ns / NSEC_PER_SEC) as i64,
        tv_nsec: (ns % NSEC_PER_SEC) as i64,
    }
}

fn ns_to_duration(ns: u64) -> Duration {
    Duration::from_secs(ns / NSEC_PER_SEC) + Duration::from_nanos(ns % NSEC_PER_SEC)
}

fn deadline_timeout(now_ns: u64, deadline_ns: u64) -> Duration {
    ns_to_duration(deadline_ns.saturating_sub(now_ns))
}

fn validate_itimerspec(spec: ITimerSpec) -> Result<(u64, Option<u64>), SysError> {
    let value_ns = timespec_to_ns(spec.it_value)?;
    let interval_ns = timespec_to_ns(spec.it_interval)?;
    let interval = (interval_ns != 0).then_some(interval_ns);
    Ok((value_ns, interval))
}

fn validate_settime_value(spec: ITimerSpec) -> Result<(), SysError> {
    validate_itimerspec(spec).map(|_| ())
}

fn snapshot_itimerspec(clockid: i32, state: &TimerFdState) -> ITimerSpec {
    let interval_ns = match state.schedule {
        TimerFdSchedule::Disarmed => 0,
        TimerFdSchedule::Armed { interval_ns, .. } => interval_ns.unwrap_or(0),
    };
    let value_ns = match state.schedule {
        TimerFdSchedule::Disarmed => 0,
        TimerFdSchedule::Armed {
            next_expire_at_ns, ..
        } => {
            let now_ns = get_clock(clockid as usize)
                .expect("validated timerfd clock disappeared")
                .now_ns();
            next_expire_at_ns.saturating_sub(now_ns)
        },
    };
    ITimerSpec {
        it_interval: ns_to_timespec(interval_ns),
        it_value: ns_to_timespec(value_ns),
    }
}

fn drop_stale_waiters(waiters: TimerFdHandoffBatch, reason: &'static str) {
    if waiters.is_empty() {
        return;
    }
    assert!(
        waiters.poll_notify.is_empty(),
        "timerfd dropped a live poll notification batch"
    );

    kdebugln!(
        "timerfd: dropped stale read={} poll={} waiters reason={}",
        waiters.read.len(),
        waiters.poll_stale.len(),
        reason,
    );
}

fn notify_waiters_after_unlock(waiters: TimerFdHandoffBatch, reason: &'static str) {
    if waiters.is_empty() {
        return;
    }

    kdebugln!(
        "timerfd: detached read={} poll_notify={} poll_stale={} waiters reason={}",
        waiters.read.len(),
        waiters.poll_notify.len(),
        waiters.poll_stale.len(),
        reason,
    );

    for trigger in waiters.read {
        kdebugln!(
            "timerfd: trigger read wait={:#x} reason={}",
            trigger.trigger.wait_id(),
            reason,
        );
        trigger.trigger.trigger();
    }

    for route in waiters.poll_notify {
        kdebugln!(
            "timerfd: notify poll route interests={:?} reason={}",
            route.interests,
            reason,
        );
        route.route.notify();
    }
}

fn schedule_timerfd_callback(core: &Arc<TimerFdCore>, generation: u64, timeout: Duration) {
    let weak = Arc::downgrade(core);
    // Timerfd submits a bounded threaded completion, not a background job. The
    // timerfd object still owns generation filtering, missed-tick accounting,
    // trigger handoff and periodic rearm under its state lock. Callers may use
    // this before unlocking because this RFC's threaded timer submit has no
    // recoverable failure path; normal return is the queued-event publish point.
    schedule_threaded_timer_event(
        timeout,
        Box::new(move || timerfd_expire_callback(weak, generation)),
    );
}

fn timerfd_expire_callback(core: Weak<TimerFdCore>, generation: u64) {
    let Some(core) = core.upgrade() else {
        return;
    };

    let detached = {
        let mut state = core.state.lock();
        if state.generation != generation {
            return;
        }

        let TimerFdSchedule::Armed {
            next_expire_at_ns,
            interval_ns,
        } = state.schedule
        else {
            return;
        };

        let (detached, timeout) = account_due_expiration_locked(
            &mut state,
            core.now_ns(),
            next_expire_at_ns,
            interval_ns,
        );
        if let Some(timeout) = timeout {
            // Submit the successor event before publishing the updated armed
            // state by unlocking. This keeps the ordinary path from exposing an
            // armed periodic timer without a matching queued timer-core event.
            schedule_timerfd_callback(&core, generation, timeout);
        }
        detached
    };

    notify_waiters_after_unlock(detached, "expire");
}

fn refresh_due_expiration_locked(
    core: &Arc<TimerFdCore>,
    state: &mut TimerFdState,
) -> TimerFdHandoffBatch {
    let TimerFdSchedule::Armed {
        next_expire_at_ns,
        interval_ns,
    } = state.schedule
    else {
        return TimerFdHandoffBatch::empty();
    };

    let now_ns = core.now_ns();
    if now_ns < next_expire_at_ns {
        return TimerFdHandoffBatch::empty();
    }

    // Read/poll readiness is derived from the timerfd object's clock state, not
    // solely from whether the threaded callback has already run. If a reader
    // observes an overdue timer before the queued completion gets CPU time,
    // advance the object state here and make that queued completion stale.
    state.generation = state.generation.wrapping_add(1);
    let generation = state.generation;
    let (detached, timeout) =
        account_due_expiration_locked(state, now_ns, next_expire_at_ns, interval_ns);
    if let Some(timeout) = timeout {
        schedule_timerfd_callback(core, generation, timeout);
    }
    detached
}

fn account_due_expiration_locked(
    state: &mut TimerFdState,
    now_ns: u64,
    next_expire_at_ns: u64,
    interval_ns: Option<u64>,
) -> (TimerFdHandoffBatch, Option<Duration>) {
    if now_ns < next_expire_at_ns {
        return (
            TimerFdHandoffBatch::empty(),
            Some(deadline_timeout(now_ns, next_expire_at_ns)),
        );
    }

    if let Some(interval_ns) = interval_ns {
        let elapsed = now_ns.saturating_sub(next_expire_at_ns);
        let ticks = (elapsed / interval_ns).saturating_add(1);
        state.expirations = state.expirations.saturating_add(ticks);
        let advanced = interval_ns.saturating_mul(ticks);
        let next_expire_at_ns = next_expire_at_ns.saturating_add(advanced);
        state.schedule = TimerFdSchedule::Armed {
            next_expire_at_ns,
            interval_ns: Some(interval_ns),
        };
        let timeout = deadline_timeout(now_ns, next_expire_at_ns);
        (state.collect_readable_waiters(), Some(timeout))
    } else {
        state.expirations = state.expirations.saturating_add(1);
        state.schedule = TimerFdSchedule::Disarmed;
        (state.collect_readable_waiters(), None)
    }
}

fn timerfd_wait_for_readable(timerfd: &TimerFdFile) -> Result<(), SysError> {
    loop {
        if get_current_task().has_unmasked_signal() {
            return Err(SysError::Interrupted);
        }

        let latch = Latch::begin_current(true);
        let trigger = latch.make_trigger();

        let mut stale = TimerFdHandoffBatch::empty();
        let (register_result, due, ready) = {
            let mut state = timerfd.core.state.lock();
            let due = refresh_due_expiration_locked(&timerfd.core, &mut state);
            if state.expirations > 0 {
                (Ok(()), due, true)
            } else {
                (state.register_read_wait(&trigger, &mut stale), due, false)
            }
        };
        notify_waiters_after_unlock(due, "read_refresh");
        if ready {
            latch.cancel(LatchCancelReason::PredicateReady);
            let outcome = latch.finish();
            kdebugln!(
                "timerfd: read wait found readable before sleep outcome={:?}",
                outcome,
            );
            return Ok(());
        }
        drop_stale_waiters(stale, "read_register");
        if let Err(err) = register_result {
            latch.cancel(LatchCancelReason::RegisterError);
            let outcome = latch.finish();
            kwarningln!(
                "timerfd: failed to arm read wait outcome={:?} err={:?}",
                outcome,
                err,
            );
            return Err(err);
        }

        latch.schedule_with_timeout(None);
        let outcome = latch.finish();
        match outcome {
            LatchWaitOutcome::Triggered => return Ok(()),
            LatchWaitOutcome::Signal | LatchWaitOutcome::Force => {
                return Err(SysError::Interrupted);
            },
            LatchWaitOutcome::Cancelled | LatchWaitOutcome::Unexpected => {
                kwarningln!("timerfd: unexpected read wait outcome={:?}", outcome);
                return Err(SysError::IO);
            },
            LatchWaitOutcome::Timeout => {
                kwarningln!("timerfd: blocking read wait timed out without timeout");
                return Err(SysError::IO);
            },
        }
    }
}

fn timerfd_read(
    file: &File,
    _pos: &mut usize,
    buf: &mut [u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    if buf.len() < size_of::<u64>() {
        return Err(SysError::InvalidArgument);
    }

    let timerfd = TimerFdFile::from_file(file).expect("timerfd file without timerfd private data");
    loop {
        let (value, due) = {
            let mut state = timerfd.core.state.lock();
            let due = refresh_due_expiration_locked(&timerfd.core, &mut state);
            let value = if state.expirations == 0 {
                None
            } else {
                let value = state.expirations;
                state.expirations = 0;
                Some(value)
            };
            (value, due)
        };
        notify_waiters_after_unlock(due, "read_refresh");

        if let Some(value) = value {
            buf[..size_of::<u64>()].copy_from_slice(&value.to_le_bytes());
            return Ok(size_of::<u64>());
        }

        if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
            return Err(SysError::Again);
        }
        timerfd_wait_for_readable(timerfd)?;
    }
}

fn timerfd_poll(file: &File, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
    let timerfd = TimerFdFile::from_file(file).expect("timerfd file without timerfd private data");

    let mut stale = TimerFdHandoffBatch::empty();
    let (result, due, capacity_exhausted) = {
        let mut state = timerfd.core.state.lock();
        let due = refresh_due_expiration_locked(&timerfd.core, &mut state);
        let revents = state.revents(request.interests());
        let mut capacity_exhausted = false;
        let result = if !request.is_register() {
            PollRegisterResult::Ready(revents)
        } else if !request.interests().contains(PollEvent::READABLE) {
            PollRegisterResult::Unsupported
        } else if let Some(route) = request.route() {
            if state.register_poll_route(route, request.interests(), &mut stale) {
                PollRegisterResult::Subscribed(revents)
            } else {
                capacity_exhausted = true;
                PollRegisterResult::Unsupported
            }
        } else {
            PollRegisterResult::Unsupported
        };
        (result, due, capacity_exhausted)
    };
    notify_waiters_after_unlock(due, "poll_refresh");
    drop_stale_waiters(stale, "poll_register");
    if capacity_exhausted {
        kwarningln!(
            "timerfd: poll route capacity exhausted capacity={}",
            TIMERFD_TRIGGER_QUEUE_CAPACITY,
        );
    }
    Ok(result)
}

fn timerfd_check_status_flags(_file: &File, flags: FileOpStatusFlags) -> Result<(), SysError> {
    if flags.contains(FileOpStatusFlags::DIRECT) {
        knoticeln!("timerfd: rejecting O_DIRECT status flag");
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

static TIMERFD_FILE_OPS: FileOps = FileOps {
    read: timerfd_read,
    write: |_, _, _, _| Err(SysError::InvalidArgument),
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: timerfd_check_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: timerfd_poll,
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

fn timerfd_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
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

static TIMERFD_INODE_OPS: InodeOps = InodeOps {
    make_node: reject_make_node,
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| unreachable!("timerfd files are opened with explicit private state"),
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: timerfd_get_attr,
};

fn valid_timerfd_clockid(clockid: i32) -> bool {
    use anemone_abi::time::linux::clock::{CLOCK_BOOTTIME, CLOCK_MONOTONIC, CLOCK_REALTIME};

    matches!(clockid, CLOCK_REALTIME | CLOCK_MONOTONIC | CLOCK_BOOTTIME)
        && get_clock(clockid as usize).is_some()
}

fn create_timerfd(clockid: i32) -> Result<File, SysError> {
    if !valid_timerfd_clockid(clockid) {
        return Err(SysError::InvalidArgument);
    }

    let path = anony_new_inode(InodeType::Anon, &TIMERFD_INODE_OPS, NilOpaque::new())?;
    anony_open_with(
        &path,
        OpenedFile::with_mode(
            &TIMERFD_FILE_OPS,
            FileMode::STREAM,
            AnyOpaque::new(TimerFdFile::new(clockid)?),
        ),
    )
}

fn gettime(file: &File) -> Result<ITimerSpec, SysError> {
    let core = TimerFdFile::core_from_file(file)?;
    let state = core.state.lock();
    Ok(snapshot_itimerspec(core.clockid, &state))
}

fn settime(
    file: &File,
    flags: TimerFdSettimeFlags,
    new_value: ITimerSpec,
) -> Result<ITimerSpec, SysError> {
    let (value_ns, interval_ns) = validate_itimerspec(new_value)?;
    let core = TimerFdFile::core_from_file(file)?;
    let mut detached = TimerFdHandoffBatch::empty();

    let old_value = {
        let mut state = core.state.lock();
        let old_value = snapshot_itimerspec(core.clockid, &state);

        state.generation = state.generation.wrapping_add(1);
        state.cancel_on_set_accepted = flags.cancel_on_set;
        if flags.cancel_on_set {
            knoticeln!(
                "timerfd: TFD_TIMER_CANCEL_ON_SET accepted as stage-1 no-op; read ECANCELED is not implemented"
            );
        }
        state.expirations = 0;

        if value_ns == 0 {
            state.schedule = TimerFdSchedule::Disarmed;
        } else {
            let now_ns = core.now_ns();
            let next_expire_at_ns = if flags.abstime {
                value_ns
            } else {
                now_ns.saturating_add(value_ns)
            };
            state.schedule = TimerFdSchedule::Armed {
                next_expire_at_ns,
                interval_ns,
            };
            let (new_detached, timeout) =
                account_due_expiration_locked(&mut state, now_ns, next_expire_at_ns, interval_ns);
            detached = new_detached;
            if let Some(timeout) = timeout {
                // Normal settime has no recoverable timer-core submit failure:
                // return from schedule_threaded_timer_event() is the point that
                // lets this armed generation become visible to readers.
                schedule_timerfd_callback(&core, state.generation, timeout);
            }
        }

        old_value
    };

    notify_waiters_after_unlock(detached, "settime");

    Ok(old_value)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::fs::iomux::IomuxWaitRound;
    use anemone_abi::time::linux::clock::CLOCK_MONOTONIC;

    #[kunit]
    fn poll_route_notifies_after_unlock_and_reuses_stale_capacity() {
        let file = create_timerfd(CLOCK_MONOTONIC).unwrap();
        let core = TimerFdFile::core_from_file(&file).unwrap();

        let ready_round = IomuxWaitRound::begin_current();
        let ready_request = ready_round.poll_request(PollEvent::READABLE);
        {
            let mut state = core.state.lock();
            state.expirations = 1;
        }
        assert_eq!(
            file.poll(&ready_request).unwrap(),
            PollRegisterResult::Subscribed(PollEvent::READABLE)
        );
        assert_eq!(core.state.lock().poll_routes.len(), 1);
        ready_round.cancel(LatchCancelReason::PredicateReady);
        let _ = ready_round.finish();

        let notified_round = IomuxWaitRound::begin_current();
        let notified_request = notified_round.poll_request(PollEvent::READABLE);
        {
            let mut state = core.state.lock();
            state.expirations = 0;
        }
        for _ in 0..TIMERFD_TRIGGER_QUEUE_CAPACITY {
            assert_eq!(
                file.poll(&notified_request).unwrap(),
                PollRegisterResult::Subscribed(PollEvent::empty())
            );
        }
        assert_eq!(
            core.state.lock().poll_routes.len(),
            TIMERFD_TRIGGER_QUEUE_CAPACITY
        );
        assert_eq!(
            file.poll(&notified_request).unwrap(),
            PollRegisterResult::Unsupported
        );

        let detached = {
            let mut state = core.state.lock();
            account_due_expiration_locked(&mut state, 1, 1, None).0
        };
        assert_eq!(detached.poll_notify.len(), TIMERFD_TRIGGER_QUEUE_CAPACITY);
        assert!(detached.poll_stale.is_empty());
        assert_eq!(
            core.state.lock().poll_routes.len(),
            TIMERFD_TRIGGER_QUEUE_CAPACITY
        );
        notify_waiters_after_unlock(detached, "kunit_expire");

        notified_round.schedule_with_timeout(Some(Duration::from_secs(1)));
        assert_eq!(notified_round.finish(), LatchWaitOutcome::Triggered);

        let reused_round = IomuxWaitRound::begin_current();
        let reused_request = reused_round.poll_request(PollEvent::READABLE);
        core.state.lock().expirations = 0;
        assert_eq!(
            file.poll(&reused_request).unwrap(),
            PollRegisterResult::Subscribed(PollEvent::empty())
        );
        assert_eq!(core.state.lock().poll_routes.len(), 1);
        reused_round.cancel(LatchCancelReason::PredicateReady);
        let _ = reused_round.finish();
    }
}
