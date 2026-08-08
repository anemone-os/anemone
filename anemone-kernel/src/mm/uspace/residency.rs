//! User-address-space activation and TLB residency ownership.

use super::*;
use crate::{
    exception::ipi::user_tlb::{PreparedUserTlbShootdown, UserTlbShootdownSet},
    mm::kptable::activate_kernel_mapping,
};

#[derive(Debug)]
struct ResidentCpus {
    members: Vec<bool>,
}

impl ResidentCpus {
    fn new() -> Self {
        Self {
            members: vec![false; ncpus()],
        }
    }

    fn join(&mut self, cpu: CpuId) {
        let member = &mut self.members[cpu.logical_id()];
        assert!(!*member, "CPU joined one user address space twice");
        *member = true;
    }

    fn leave(&mut self, cpu: CpuId) {
        let member = &mut self.members[cpu.logical_id()];
        assert!(*member, "CPU left a user address space without residency");
        *member = false;
    }

    fn contains(&self, cpu: CpuId) -> bool {
        self.members[cpu.logical_id()]
    }

    fn is_empty(&self) -> bool {
        !self.members.iter().any(|member| *member)
    }
}

/// The sole behavioral truth for CPUs that may still use this address space's
/// hardware translations.
///
/// The lock is also the join/snapshot ordering point. Activation installs the
/// root and completes a local full invalidation before publishing residency.
/// A destructive transaction snapshots targets only after its mutation and
/// current-core completion. Therefore a join is either selected by that
/// snapshot or performs its local invalidation after the snapshot's release.
#[derive(Debug)]
pub(super) struct UserTlbResidency {
    residents: SpinLock<ResidentCpus>,
}

impl UserTlbResidency {
    pub(super) fn new() -> Self {
        Self {
            residents: SpinLock::new(ResidentCpus::new()),
        }
    }

    fn activate_current(&self, root_ppn: PhysPageNum) {
        let cpu = cur_cpu_id();
        assert!(
            target_online(cpu),
            "only a published online CPU may join user TLB residency"
        );
        let mut residents = self.residents.lock_irqsave();
        unsafe {
            PagingArch::activate_addr_space(root_ppn);
        }
        residents.join(cpu);
    }

    fn leave_current_after_local_destruction(&self) {
        self.residents.lock_irqsave().leave(cur_cpu_id());
    }

    pub(super) fn prepare_targets<'a>(
        &self,
        transport: &'a UserTlbShootdownSet,
        range: Option<VirtPageRange>,
    ) -> PreparedUserTlbShootdown<'a> {
        let residents = self.residents.lock_irqsave();
        transport.prepare_targets(range, |cpu| residents.contains(cpu))
    }
}

impl Drop for UserTlbResidency {
    fn drop(&mut self) {
        assert!(
            self.residents.lock_irqsave().is_empty(),
            "dropping a user address space with resident CPUs"
        );
    }
}

/// Move the current CPU between hardware mappings and update residency at the
/// same MM-owned boundary.
///
/// # Safety
///
/// `previous` must describe the user mapping currently installed on this CPU,
/// if any. Preemption must remain disabled for the whole transition.
pub(crate) unsafe fn activate_mapping_transition(
    previous: Option<&UserSpaceHandle>,
    next: Option<&UserSpaceHandle>,
) {
    assert!(
        IntrArch::local_intr_disabled() || !allow_preempt(),
        "user mapping transition requires a stable current CPU"
    );

    if matches!((previous, next), (Some(previous), Some(next)) if core::ptr::eq(previous, next)) {
        return;
    }
    if previous.is_none() && next.is_none() {
        return;
    }

    if let Some(next) = next {
        next.tlb_residency.activate_current(next.table_ppn);
    } else {
        unsafe {
            activate_kernel_mapping();
        }
    }

    if let Some(previous) = previous {
        previous
            .tlb_residency
            .leave_current_after_local_destruction();
    }
}

/// Scoped activation for kernel code that temporarily accesses another user
/// address space through the current hardware mapping.
///
/// The preemption guard pins the transition and the direct access to one CPU;
/// Drop restores the original mapping before that pin is released.
#[derive(Debug)]
pub(crate) struct TemporaryUserSpaceActivation<'a> {
    original: &'a UserSpaceHandle,
    temporary: &'a UserSpaceHandle,
    _preempt_guard: PreemptGuard,
}

impl<'a> TemporaryUserSpaceActivation<'a> {
    pub(crate) fn new(original: &'a UserSpaceHandle, temporary: &'a UserSpaceHandle) -> Self {
        assert!(
            !core::ptr::eq(original, temporary),
            "temporary activation requires a different address space"
        );
        let preempt_guard = PreemptGuard::new();
        unsafe {
            activate_mapping_transition(Some(original), Some(temporary));
        }
        Self {
            original,
            temporary,
            _preempt_guard: preempt_guard,
        }
    }
}

impl Drop for TemporaryUserSpaceActivation<'_> {
    fn drop(&mut self) {
        unsafe {
            activate_mapping_transition(Some(self.temporary), Some(self.original));
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn empty_set(count: usize) -> ResidentCpus {
        ResidentCpus {
            members: vec![false; count],
        }
    }

    #[kunit]
    fn stable_resident_set_tracks_only_joined_cpus() {
        let mut residents = empty_set(4);
        residents.join(CpuId::new(1));
        residents.join(CpuId::new(3));

        assert!(!residents.contains(CpuId::new(0)));
        assert!(residents.contains(CpuId::new(1)));
        assert!(!residents.contains(CpuId::new(2)));
        assert!(residents.contains(CpuId::new(3)));
    }

    #[kunit]
    fn completed_leave_converges_to_remaining_residents() {
        let mut residents = empty_set(3);
        residents.join(CpuId::new(0));
        residents.join(CpuId::new(2));
        residents.leave(CpuId::new(0));

        assert!(!residents.contains(CpuId::new(0)));
        assert!(residents.contains(CpuId::new(2)));
        residents.leave(CpuId::new(2));
        assert!(residents.is_empty());
    }
}
