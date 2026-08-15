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
        let (result, change) = op(self
            .usp
            .as_deref_mut()
            .expect("user-space guard must hold the inner mutex during mutation"))?;

        let Some(change) = change else {
            return Ok(result);
        };

        // The mutation and current-core completion precede this snapshot.
        // Activation joins through the same residency lock, so a later join
        // must perform its full local invalidation after this point.
        let prepared = self
            .handle
            .tlb_residency
            .prepare_targets(&self.handle.user_tlb_shootdown, range);
        let committed = prepared.commit();
        drop(
            self.usp
                .take()
                .expect("destructive completion must release the inner mutex once"),
        );
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
