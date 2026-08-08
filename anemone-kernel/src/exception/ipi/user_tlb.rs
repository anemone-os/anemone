//! Allocation-free synchronous user-address-space TLB shootdown transport.

use core::{cell::UnsafeCell, hint::spin_loop, ptr};

use alloc::{alloc::AllocError, vec::Vec};

use crate::prelude::*;

use super::IpiError;

const USER_TLB_IDLE: u8 = 0;
const USER_TLB_PREPARED: u8 = 1;
const USER_TLB_QUEUED: u8 = 2;
const USER_TLB_ACKNOWLEDGED: u8 = 3;

/// One allocation-free user-address-space TLB request for a target CPU.
///
/// A request is permanently owned by one [`UserTlbShootdownSet`]. The owning
/// address-space completion gate permits at most one prepared round at a time,
/// and the sender retains the set until every queued request is acknowledged.
/// The intrusive link is transport state only and is never used to decide MM
/// semantics.
#[derive(Debug)]
struct UserTlbShootdownMsg {
    range: UnsafeCell<Option<VirtPageRange>>,
    next: UnsafeCell<*mut UserTlbShootdownMsg>,
    phase: AtomicU8,
}

impl UserTlbShootdownMsg {
    const fn new() -> Self {
        Self {
            range: UnsafeCell::new(None),
            next: UnsafeCell::new(ptr::null_mut()),
            phase: AtomicU8::new(USER_TLB_IDLE),
        }
    }

    fn prepare(&self, range: Option<VirtPageRange>) {
        assert_eq!(
            self.phase.load(Ordering::Acquire),
            USER_TLB_IDLE,
            "user TLB request reused before its previous round completed"
        );
        unsafe {
            *self.range.get() = range;
        }
        self.phase.store(USER_TLB_PREPARED, Ordering::Release);
    }

    fn cancel(&self) {
        assert_eq!(
            self.phase.compare_exchange(
                USER_TLB_PREPARED,
                USER_TLB_IDLE,
                Ordering::AcqRel,
                Ordering::Acquire,
            ),
            Ok(USER_TLB_PREPARED),
            "only an unqueued user TLB request can be cancelled"
        );
    }

    fn mark_queued(&self) {
        assert_eq!(
            self.phase.compare_exchange(
                USER_TLB_PREPARED,
                USER_TLB_QUEUED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ),
            Ok(USER_TLB_PREPARED),
            "user TLB request must be prepared exactly once before enqueue"
        );
    }

    fn handle(&self) {
        assert_eq!(
            self.phase.load(Ordering::Acquire),
            USER_TLB_QUEUED,
            "IPI handler observed an unqueued user TLB request"
        );
        let range = unsafe { *self.range.get() };
        if let Some(range) = range {
            PagingArch::tlb_shootdown_range(range);
        } else {
            PagingArch::tlb_shootdown_all();
        }
        self.phase.store(USER_TLB_ACKNOWLEDGED, Ordering::Release);
    }

    fn wait_and_reset(&self) {
        while self.phase.load(Ordering::Acquire) != USER_TLB_ACKNOWLEDGED {
            spin_loop();
        }
        unsafe {
            *self.range.get() = None;
        }
        self.phase.store(USER_TLB_IDLE, Ordering::Release);
    }
}

// The phase machine and the target CPU's queue lock serialize every access to
// the interior fields. The sender owns mutation in Idle/Prepared, while the
// target handler owns the Queued -> Acknowledged transition.
unsafe impl Send for UserTlbShootdownMsg {}
unsafe impl Sync for UserTlbShootdownMsg {}

#[derive(Debug)]
struct UserTlbShootdownQueue {
    head: *mut UserTlbShootdownMsg,
    tail: *mut UserTlbShootdownMsg,
}

impl UserTlbShootdownQueue {
    const fn new() -> Self {
        Self {
            head: ptr::null_mut(),
            tail: ptr::null_mut(),
        }
    }

    unsafe fn push(&mut self, msg: *mut UserTlbShootdownMsg) {
        assert!(!msg.is_null());
        assert!(unsafe { (*(*msg).next.get()).is_null() });
        if self.tail.is_null() {
            self.head = msg;
        } else {
            unsafe {
                *(*self.tail).next.get() = msg;
            }
        }
        self.tail = msg;
    }

    unsafe fn pop(&mut self) -> *mut UserTlbShootdownMsg {
        let msg = self.head;
        if msg.is_null() {
            return msg;
        }
        let next = unsafe { *(*msg).next.get() };
        self.head = next;
        if next.is_null() {
            self.tail = ptr::null_mut();
        }
        unsafe {
            *(*msg).next.get() = ptr::null_mut();
        }
        msg
    }
}

// Raw links always refer to stable messages owned by live address spaces and
// are accessed only while the per-CPU queue lock is held.
unsafe impl Send for UserTlbShootdownQueue {}

#[percpu]
static USER_TLB_SHOOTDOWN_QUEUE: SpinLock<UserTlbShootdownQueue> =
    SpinLock::new(UserTlbShootdownQueue::new());

fn enqueue_user_tlb_shootdown(cpu_id: CpuId, msg: &UserTlbShootdownMsg) {
    msg.mark_queued();
    unsafe {
        USER_TLB_SHOOTDOWN_QUEUE.with_remote(cpu_id, |queue| {
            queue
                .lock_irqsave()
                .push(msg as *const UserTlbShootdownMsg as *mut UserTlbShootdownMsg);
        });
    }
}

pub(super) fn handle_user_tlb_shootdowns() {
    USER_TLB_SHOOTDOWN_QUEUE.with(|queue| {
        loop {
            let msg = unsafe { queue.lock_irqsave().pop() };
            if msg.is_null() {
                break;
            }
            unsafe {
                (*msg).handle();
            }
        }
    });
}

/// Per-address-space storage for allocation-free synchronous remote TLB rounds.
///
/// Construction is fallible and happens before the address space is published.
/// Once constructed, preparing and completing a round cannot allocate. Runtime
/// CPU hotplug is unsupported, so every round snapshots the boot-fixed online
/// set and treats a later offline transition as a correctness violation.
#[derive(Debug)]
pub(crate) struct UserTlbShootdownSet {
    messages: Vec<UserTlbShootdownMsg>,
}

impl UserTlbShootdownSet {
    pub(crate) fn try_new() -> Result<Self, IpiError> {
        Self::try_new_with_reservation(|messages, count| {
            messages.try_reserve_exact(count).map_err(|_| AllocError)
        })
    }

    fn try_new_with_reservation(
        reserve: impl FnOnce(&mut Vec<UserTlbShootdownMsg>, usize) -> Result<(), AllocError>,
    ) -> Result<Self, IpiError> {
        let mut messages = Vec::new();
        reserve(&mut messages, ncpus()).map_err(IpiError::Alloc)?;
        for _ in 0..ncpus() {
            messages.push(UserTlbShootdownMsg::new());
        }
        Ok(Self { messages })
    }

    pub(crate) fn prepare(&self, range: Option<VirtPageRange>) -> PreparedUserTlbShootdown<'_> {
        self.prepare_with_online(range, target_online)
    }

    fn prepare_with_online(
        &self,
        range: Option<VirtPageRange>,
        is_online: impl Fn(CpuId) -> bool,
    ) -> PreparedUserTlbShootdown<'_> {
        let source = cur_cpu_id();
        for (logical_id, msg) in self.messages.iter().enumerate() {
            let target = CpuId::new(logical_id);
            // An AP that has not been published online cannot hold a user TLB
            // entry yet and is therefore outside this round. Runtime hotplug
            // is unsupported; an online target disappearing after this
            // snapshot is asserted in `complete`.
            if target != source && is_online(target) {
                msg.prepare(range);
            }
        }
        PreparedUserTlbShootdown {
            set: self,
            source,
            active: true,
        }
    }

    fn cancel(&self, source: CpuId) {
        for (logical_id, msg) in self.messages.iter().enumerate() {
            if CpuId::new(logical_id) != source
                && msg.phase.load(Ordering::Acquire) == USER_TLB_PREPARED
            {
                msg.cancel();
            }
        }
    }
}

impl Drop for UserTlbShootdownSet {
    fn drop(&mut self) {
        assert!(
            self.messages
                .iter()
                .all(|msg| msg.phase.load(Ordering::Acquire) == USER_TLB_IDLE),
            "dropping an address-space TLB set with an active remote round"
        );
    }
}

#[derive(Debug)]
pub(crate) struct PreparedUserTlbShootdown<'a> {
    set: &'a UserTlbShootdownSet,
    source: CpuId,
    active: bool,
}

impl<'a> PreparedUserTlbShootdown<'a> {
    pub(crate) fn commit(mut self) -> CommittedUserTlbShootdown<'a> {
        self.active = false;
        CommittedUserTlbShootdown {
            set: self.set,
            source: self.source,
            completed: false,
        }
    }
}

impl Drop for PreparedUserTlbShootdown<'_> {
    fn drop(&mut self) {
        if self.active {
            self.set.cancel(self.source);
        }
    }
}

#[derive(Debug)]
pub(crate) struct CommittedUserTlbShootdown<'a> {
    set: &'a UserTlbShootdownSet,
    source: CpuId,
    completed: bool,
}

impl CommittedUserTlbShootdown<'_> {
    pub(crate) fn complete(mut self) {
        assert_eq!(
            cur_cpu_id(),
            self.source,
            "user TLB mutation and remote completion must stay on one CPU"
        );
        for (logical_id, msg) in self.set.messages.iter().enumerate() {
            let target = CpuId::new(logical_id);
            if target == self.source || msg.phase.load(Ordering::Acquire) != USER_TLB_PREPARED {
                continue;
            }
            assert!(
                target_online(target),
                "boot-fixed CPU became offline during user TLB completion"
            );
            enqueue_user_tlb_shootdown(target, msg);
            IntrArch::send_ipi(target.physical_id());
        }
        for (logical_id, msg) in self.set.messages.iter().enumerate() {
            if CpuId::new(logical_id) != self.source
                && msg.phase.load(Ordering::Acquire) != USER_TLB_IDLE
            {
                msg.wait_and_reset();
            }
        }
        self.completed = true;
    }
}

impl Drop for CommittedUserTlbShootdown<'_> {
    fn drop(&mut self) {
        assert!(
            self.completed,
            "destructive user mapping escaped without remote TLB completion"
        );
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn injected_storage_failure_constructs_no_user_tlb_transport() {
        let result = UserTlbShootdownSet::try_new_with_reservation(|_, _| Err(AllocError));
        assert!(matches!(result, Err(IpiError::Alloc(_))));
    }

    #[kunit]
    fn preparation_snapshots_only_targets_already_online() {
        if ncpus() < 2 {
            return;
        }
        let set = UserTlbShootdownSet::try_new().expect("transport allocation should succeed");
        let source = cur_cpu_id();
        let skipped = (0..ncpus())
            .map(CpuId::new)
            .find(|cpu| *cpu != source)
            .expect("SMP KUnit requires one remote CPU");

        let prepared = set.prepare_with_online(None, |target| target != skipped);
        for (logical_id, msg) in set.messages.iter().enumerate() {
            let target = CpuId::new(logical_id);
            let expected = if target == source || target == skipped {
                USER_TLB_IDLE
            } else {
                USER_TLB_PREPARED
            };
            assert_eq!(msg.phase.load(Ordering::Acquire), expected);
        }
        drop(prepared);
        assert!(
            set.messages
                .iter()
                .all(|msg| msg.phase.load(Ordering::Acquire) == USER_TLB_IDLE)
        );
    }
}
