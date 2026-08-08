//! User-address-space remote completion and retirement capabilities.

use core::ops::{Deref, DerefMut};

use super::{vmo::RetiredFrames, *};

/// Resources whose release must follow one destructive remote TLB completion.
#[derive(Debug, Default)]
pub(super) struct UserTlbRetirement {
    page_tables: Vec<OwnedFrameHandle>,
    frames: RetiredFrames,
    vmas: Vec<VmArea>,
}

impl UserTlbRetirement {
    pub(super) fn page_tables(&mut self) -> &mut Vec<OwnedFrameHandle> {
        &mut self.page_tables
    }

    pub(super) fn frames(&mut self) -> &mut RetiredFrames {
        &mut self.frames
    }

    pub(super) fn keep_vma(&mut self, vma: VmArea) {
        self.vmas.push(vma);
    }

    pub(super) fn keep_vmas(&mut self, vmas: BTreeMap<VirtPageNum, VmArea>) {
        self.vmas.extend(vmas.into_values());
    }
}

/// Linear proof that a user mapping mutation requires remote completion.
///
/// An empty retirement set is still meaningful: replacement or restriction
/// changes translation semantics even when no frame or backing becomes free.
#[derive(Debug)]
pub(super) struct DestructiveUserTlbChange {
    retirement: UserTlbRetirement,
}

impl DestructiveUserTlbChange {
    pub(super) fn new(retirement: UserTlbRetirement) -> Self {
        Self { retirement }
    }

    pub(super) fn without_retirement() -> Self {
        Self::new(UserTlbRetirement::default())
    }
}

/// The only mutable access capability for a published user address space.
///
/// `ordering` serializes every continuation against an in-flight destructive
/// completion. `usp` may be released while that completion waits for remote
/// acknowledgement; architecture accessors therefore never unlock a raw mutex
/// whose lifecycle they do not own.
#[derive(Debug)]
pub struct UserSpaceGuard<'a> {
    // Drop the inner mutex before the ordering mutex on ordinary scope exit.
    usp: Option<MutexGuard<'a, UserSpace>>,
    ordering: MutexGuard<'a, ()>,
    handle: &'a UserSpaceHandle,
}

impl<'a> UserSpaceGuard<'a> {
    pub(super) fn new(handle: &'a UserSpaceHandle) -> Self {
        let ordering = handle.completion_ordering.lock();
        let usp = Some(handle.usp.lock());
        Self {
            usp,
            ordering,
            handle,
        }
    }

    /// Prepare before mutation, then explicitly complete any destructive
    /// outcome outside the `UserSpace` mutex before returning to the caller.
    pub(super) fn run_tlb_transaction<R>(
        &mut self,
        range: Option<VirtPageRange>,
        op: impl FnOnce(&mut UserSpace) -> Result<(R, Option<DestructiveUserTlbChange>), SysError>,
    ) -> Result<R, SysError> {
        let prepared = self.handle.user_tlb_shootdown.prepare(range);
        let (result, change) = op(self
            .usp
            .as_deref_mut()
            .expect("user-space guard must hold the inner mutex during mutation"))?;

        let Some(change) = change else {
            drop(prepared);
            return Ok(result);
        };

        let committed = prepared.commit();
        drop(
            self.usp
                .take()
                .expect("destructive completion must release the inner mutex once"),
        );
        #[cfg(feature = "kunit")]
        kunits::before_remote_completion_for_kunit(self.handle);
        committed.complete();
        // Retired resources are intentionally released only after ack.
        drop(change.retirement);
        self.usp = Some(self.handle.usp.lock());
        Ok(result)
    }

    pub(crate) fn resolve_immediate_page_fault(
        &mut self,
        fault_info: &PageFaultInfo,
    ) -> Result<(), SysError> {
        let range = Some(VirtPageRange::new(fault_info.fault_addr().page_down(), 1));
        self.run_tlb_transaction(range, |usp| {
            usp.resolve_page_access(
                fault_info.fault_addr(),
                fault_info.fault_type(),
                PageAccessContinuation::Immediate,
            )
            .map(|change| ((), change))
        })
    }

    pub(crate) fn fault_in_page(
        &mut self,
        addr: VirtAddr,
        access: PageFaultType,
    ) -> Result<(), SysError> {
        let range = Some(VirtPageRange::new(addr.page_down(), 1));
        self.run_tlb_transaction(range, |usp| {
            usp.resolve_page_access(addr, access, PageAccessContinuation::Immediate)
                .map(|change| ((), change))
        })
    }
}

impl Deref for UserSpaceGuard<'_> {
    type Target = UserSpace;

    fn deref(&self) -> &Self::Target {
        self.usp
            .as_deref()
            .expect("user-space guard must hold the inner mutex outside completion")
    }
}

impl DerefMut for UserSpaceGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.usp
            .as_deref_mut()
            .expect("user-space guard must hold the inner mutex outside completion")
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{
        fs::root_pathref,
        mm::uspace::mmap::AnonymousMapping,
        task::kthread::{KThreadBuilder, KThreadCtx},
        utils::any_opaque::AnyOpaque,
    };

    static PAUSE_TARGET: AtomicUsize = AtomicUsize::new(0);
    static PAUSE_ARMED: AtomicBool = AtomicBool::new(false);
    static PAUSE_HOLD: AtomicBool = AtomicBool::new(false);
    static PAUSE_REACHED: AtomicBool = AtomicBool::new(false);

    pub(super) fn before_remote_completion_for_kunit(handle: &UserSpaceHandle) {
        if PAUSE_TARGET.load(Ordering::Acquire) != handle as *const UserSpaceHandle as usize
            || !PAUSE_ARMED.swap(false, Ordering::AcqRel)
        {
            return;
        }

        PAUSE_REACHED.store(true, Ordering::Release);
        while PAUSE_HOLD.load(Ordering::Acquire) {
            yield_now();
        }
    }

    fn arm(handle: &Arc<UserSpaceHandle>, hold: bool) {
        assert!(!PAUSE_ARMED.load(Ordering::Acquire));
        assert!(!PAUSE_HOLD.load(Ordering::Acquire));
        PAUSE_REACHED.store(false, Ordering::Release);
        PAUSE_TARGET.store(Arc::as_ptr(handle) as usize, Ordering::Release);
        PAUSE_HOLD.store(hold, Ordering::Release);
        PAUSE_ARMED.store(true, Ordering::Release);
    }

    fn release() {
        PAUSE_HOLD.store(false, Ordering::Release);
    }

    fn disarm() {
        PAUSE_ARMED.store(false, Ordering::Release);
        PAUSE_HOLD.store(false, Ordering::Release);
        PAUSE_TARGET.store(0, Ordering::Release);
    }

    fn wait_until(flag: &AtomicBool) {
        while !flag.load(Ordering::Acquire) {
            yield_now();
        }
    }

    fn worker_cpus() -> Option<(CpuId, CpuId)> {
        let mut cpus = (0..ncpus())
            .map(CpuId::new)
            .filter(|cpu| *cpu != cur_cpu_id() && target_online(*cpu));
        Some((cpus.next()?, cpus.next()?))
    }

    fn fixed_mapping(start: VirtPageNum, prot: Protection, clobber: bool) -> AnonymousMapping {
        AnonymousMapping {
            hint: Some((start, true)),
            clobber,
            npages: 1,
            prot,
            shared: false,
            flags: VmFlags::empty(),
        }
    }

    fn new_handle() -> Arc<UserSpaceHandle> {
        Arc::new(
            UserSpaceHandle::new(
                UserSpace::new().expect("user space setup should succeed"),
                root_pathref(),
            )
            .expect("user TLB transport preparation should succeed"),
        )
    }

    #[derive(Clone, Copy)]
    enum DestructiveAction {
        FixedReplace(VirtPageNum),
        Fork,
        Protect(VirtPageRange),
        HeapShrink(VirtAddr),
    }

    #[derive(Opaque)]
    struct DestructiveWorker {
        handle: Arc<UserSpaceHandle>,
        action: DestructiveAction,
        done: Arc<AtomicBool>,
    }

    fn destructive_worker(_: KThreadCtx, opaque: AnyOpaque) -> i32 {
        let worker = opaque
            .cast::<DestructiveWorker>()
            .expect("invalid destructive-completion KUnit context");
        match worker.action {
            DestructiveAction::FixedReplace(start) => {
                worker
                    .handle
                    .map_anonymous(&fixed_mapping(
                        start,
                        Protection::READ | Protection::WRITE,
                        true,
                    ))
                    .expect("fixed replacement should succeed");
            },
            DestructiveAction::Fork => {
                drop(worker.handle.fork().expect("COW fork should succeed"));
            },
            DestructiveAction::Protect(range) => {
                worker
                    .handle
                    .protect_range(range, Protection::READ)
                    .expect("permission restriction should succeed");
            },
            DestructiveAction::HeapShrink(brk) => {
                worker
                    .handle
                    .set_brk(brk)
                    .expect("heap shrink should succeed");
            },
        }
        worker.done.store(true, Ordering::Release);
        0
    }

    #[derive(Clone, Copy)]
    enum SuccessorAction {
        UserFault(VirtAddr),
        ImmediateWrite(VirtAddr),
        RejectedImmediateWrite(VirtAddr, SysError),
        Map(VirtPageNum),
    }

    #[derive(Opaque)]
    struct SuccessorWorker {
        handle: Arc<UserSpaceHandle>,
        action: SuccessorAction,
        attempted: Arc<AtomicBool>,
        continued: Arc<AtomicBool>,
    }

    fn successor_worker(_: KThreadCtx, opaque: AnyOpaque) -> i32 {
        let worker = opaque
            .cast::<SuccessorWorker>()
            .expect("invalid successor KUnit context");
        worker.attempted.store(true, Ordering::Release);
        match worker.action {
            SuccessorAction::UserFault(addr) => worker
                .handle
                .resolve_user_page_fault(&PageFaultInfo::new(
                    VirtAddr::new(0),
                    addr,
                    PageFaultType::Read,
                ))
                .expect("post-replacement user fault should succeed"),
            SuccessorAction::ImmediateWrite(addr) => worker
                .handle
                .lock()
                .fault_in_page(addr, PageFaultType::Write)
                .expect("post-fork COW fault should succeed"),
            SuccessorAction::RejectedImmediateWrite(addr, expected) => {
                assert_eq!(
                    worker
                        .handle
                        .lock()
                        .fault_in_page(addr, PageFaultType::Write),
                    Err(expected)
                );
            },
            SuccessorAction::Map(start) => {
                worker
                    .handle
                    .map_anonymous(&fixed_mapping(start, Protection::READ, false))
                    .expect("successor mapping should succeed");
            },
        }
        worker.continued.store(true, Ordering::Release);
        0
    }

    fn forced_round(
        handle: Arc<UserSpaceHandle>,
        destructive: DestructiveAction,
        successor: SuccessorAction,
    ) {
        let Some((destructive_cpu, successor_cpu)) = worker_cpus() else {
            return;
        };
        let destructive_done = Arc::new(AtomicBool::new(false));
        let attempted = Arc::new(AtomicBool::new(false));
        let continued = Arc::new(AtomicBool::new(false));

        arm(&handle, true);
        let destructive_worker = KThreadBuilder::new("kunit:user-tlb-destructive")
            .cpu(destructive_cpu)
            .spawn(
                destructive_worker,
                AnyOpaque::new(DestructiveWorker {
                    handle: handle.clone(),
                    action: destructive,
                    done: destructive_done.clone(),
                }),
            )
            .expect("failed to spawn destructive user-TLB worker");
        wait_until(&PAUSE_REACHED);

        // The production transaction has released only the PTE/VMA mutex. Its
        // outer completion-ordering mutex remains held until remote ack.
        drop(handle.usp.lock());

        let successor_worker = KThreadBuilder::new("kunit:user-tlb-successor")
            .cpu(successor_cpu)
            .spawn(
                successor_worker,
                AnyOpaque::new(SuccessorWorker {
                    handle: handle.clone(),
                    action: successor,
                    attempted: attempted.clone(),
                    continued: continued.clone(),
                }),
            )
            .expect("failed to spawn successor user-TLB worker");
        wait_until(&attempted);
        for _ in 0..32 {
            yield_now();
        }
        assert!(
            !continued.load(Ordering::Acquire),
            "successor continuation crossed an unacknowledged destructive commit"
        );

        release();
        assert_eq!(destructive_worker.wait_exited(), 0);
        assert_eq!(successor_worker.wait_exited(), 0);
        assert!(destructive_done.load(Ordering::Acquire));
        assert!(continued.load(Ordering::Acquire));
        disarm();
    }

    #[kunit]
    fn fixed_replace_blocks_added_user_fault_continuation_until_ack() {
        let handle = new_handle();
        let base = handle.lock().stack_vma().range().start() - 160;
        handle
            .map_anonymous(&fixed_mapping(base, Protection::READ, false))
            .expect("initial restrictive mapping should succeed");
        handle
            .lock()
            .fault_in_page(base.to_virt_addr(), PageFaultType::Read)
            .expect("initial restrictive leaf should resolve");

        forced_round(
            handle,
            DestructiveAction::FixedReplace(base),
            SuccessorAction::UserFault(base.to_virt_addr()),
        );
    }

    #[kunit]
    fn fork_restriction_blocks_immediate_cow_retry_until_ack() {
        let handle = new_handle();
        let base = handle.lock().stack_vma().range().start() - 176;
        handle
            .map_anonymous(&fixed_mapping(
                base,
                Protection::READ | Protection::WRITE,
                false,
            ))
            .expect("initial private mapping should succeed");
        handle
            .lock()
            .fault_in_page(base.to_virt_addr(), PageFaultType::Write)
            .expect("initial writable leaf should resolve");

        forced_round(
            handle,
            DestructiveAction::Fork,
            SuccessorAction::ImmediateWrite(base.to_virt_addr()),
        );
    }

    #[kunit]
    fn permission_restriction_blocks_successor_mapping_until_ack() {
        let handle = new_handle();
        let base = handle.lock().stack_vma().range().start() - 192;
        handle
            .map_anonymous(&fixed_mapping(
                base,
                Protection::READ | Protection::WRITE,
                false,
            ))
            .expect("initial writable mapping should succeed");
        handle
            .lock()
            .fault_in_page(base.to_virt_addr(), PageFaultType::Write)
            .expect("initial writable leaf should resolve");

        forced_round(
            handle,
            DestructiveAction::Protect(VirtPageRange::new(base, 1)),
            SuccessorAction::Map(base - 2),
        );
    }

    #[kunit]
    fn heap_decommit_retains_frame_and_blocks_fault_in_until_ack() {
        let Some((destructive_cpu, successor_cpu)) = worker_cpus() else {
            return;
        };
        let handle = new_handle();
        let heap_start = handle.lock().heap.svpn;
        let target = heap_start + 1;
        handle
            .set_brk((heap_start + 2).to_virt_addr())
            .expect("heap growth should succeed");
        handle
            .lock()
            .fault_in_page(target.to_virt_addr(), PageFaultType::Write)
            .expect("heap page should resolve");
        let old_ppn = handle
            .lock()
            .page_table_mut()
            .mapper()
            .translate(target)
            .expect("heap leaf must be present")
            .ppn;

        let destructive_done = Arc::new(AtomicBool::new(false));
        let attempted = Arc::new(AtomicBool::new(false));
        let continued = Arc::new(AtomicBool::new(false));
        arm(&handle, true);
        let destructive_worker = KThreadBuilder::new("kunit:user-tlb-heap-shrink")
            .cpu(destructive_cpu)
            .spawn(
                destructive_worker,
                AnyOpaque::new(DestructiveWorker {
                    handle: handle.clone(),
                    action: DestructiveAction::HeapShrink(target.to_virt_addr()),
                    done: destructive_done.clone(),
                }),
            )
            .expect("failed to spawn heap-shrink worker");
        wait_until(&PAUSE_REACHED);
        assert_eq!(unsafe { get_frame_raw(old_ppn) }.rc(), 1);

        let successor_worker = KThreadBuilder::new("kunit:user-tlb-heap-successor")
            .cpu(successor_cpu)
            .spawn(
                successor_worker,
                AnyOpaque::new(SuccessorWorker {
                    handle: handle.clone(),
                    action: SuccessorAction::RejectedImmediateWrite(
                        target.to_virt_addr(),
                        SysError::NotMapped,
                    ),
                    attempted: attempted.clone(),
                    continued: continued.clone(),
                }),
            )
            .expect("failed to spawn heap successor worker");
        wait_until(&attempted);
        for _ in 0..32 {
            yield_now();
        }
        assert!(!continued.load(Ordering::Acquire));

        release();
        assert_eq!(destructive_worker.wait_exited(), 0);
        assert_eq!(successor_worker.wait_exited(), 0);
        assert!(destructive_done.load(Ordering::Acquire));
        assert!(continued.load(Ordering::Acquire));
        assert_eq!(unsafe { get_frame_raw(old_ppn) }.rc(), 0);
        disarm();
    }

    #[kunit]
    fn additive_unchanged_and_relaxed_commits_do_not_start_remote_completion() {
        let handle = new_handle();
        let base = handle.lock().stack_vma().range().start() - 208;
        handle
            .map_anonymous(&fixed_mapping(base, Protection::READ, false))
            .expect("initial mapping should succeed");

        arm(&handle, false);
        handle
            .lock()
            .fault_in_page(base.to_virt_addr(), PageFaultType::Read)
            .expect("additive fault should succeed");
        handle
            .lock()
            .fault_in_page(base.to_virt_addr(), PageFaultType::Read)
            .expect("unchanged fault should succeed");
        handle
            .protect_range(
                VirtPageRange::new(base, 1),
                Protection::READ | Protection::WRITE,
            )
            .expect("permission relaxation should succeed");
        assert!(
            !PAUSE_REACHED.load(Ordering::Acquire),
            "monotonic/no-change operation created a remote completion round"
        );
        disarm();
    }
}
