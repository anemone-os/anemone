mod api;

use crate::{
    prelude::*,
    task::files::{OpenedDescriptionCapability, OpenedDescriptionLease},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlockMode {
    Shared,
    Exclusive,
}

impl FlockMode {
    fn conflicts_with(self, requested: Self) -> bool {
        self == Self::Exclusive || requested == Self::Exclusive
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlockOperation {
    Lock { mode: FlockMode, nonblocking: bool },
    Unlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlockOutcome {
    Complete,
    WouldBlock,
    Interrupted,
    Retired,
}

#[derive(Debug)]
struct FlockGrant {
    owner: OpenedDescriptionCapability,
    mode: FlockMode,
}

#[derive(Debug)]
pub(super) struct FlockDomain {
    /// Sole truth for holder-to-mode relations on this inode.
    grants: SpinLock<Vec<FlockGrant>>,
    recheck: Event,
}

enum LockAttempt {
    Complete,
    Converted(FlockGrant),
    Wait,
    WouldBlock,
    Retired,
}

impl FlockDomain {
    pub(super) const fn new() -> Self {
        Self {
            grants: SpinLock::new(Vec::new()),
            recheck: Event::new(),
        }
    }

    fn owner_index(grants: &[FlockGrant], owner: &OpenedDescriptionCapability) -> Option<usize> {
        let mut found = None;
        for (index, grant) in grants.iter().enumerate() {
            if grant.owner.same_identity(owner) {
                assert!(found.is_none(), "flock owner has duplicate inode grants");
                found = Some(index);
            }
        }
        found
    }

    fn conflicts(
        grants: &[FlockGrant],
        owner: &OpenedDescriptionCapability,
        requested: FlockMode,
    ) -> bool {
        grants
            .iter()
            .any(|grant| !grant.owner.same_identity(owner) && grant.mode.conflicts_with(requested))
    }

    fn attempt_lock(
        &self,
        owner: &OpenedDescriptionCapability,
        lease: &OpenedDescriptionLease,
        mode: FlockMode,
        nonblocking: bool,
    ) -> LockAttempt {
        let mut grants = self.grants.lock();
        if !lease.is_live() {
            return LockAttempt::Retired;
        }

        if let Some(index) = Self::owner_index(&grants, owner) {
            if grants[index].mode == mode {
                return LockAttempt::Complete;
            }
            return LockAttempt::Converted(grants.swap_remove(index));
        }

        if Self::conflicts(&grants, owner, mode) {
            if nonblocking {
                LockAttempt::WouldBlock
            } else {
                LockAttempt::Wait
            }
        } else {
            // Commit-time liveness and conflict truth are both protected by
            // this domain guard. A retirement racing after the liveness load
            // must acquire the same guard and remove this grant before close
            // returns.
            grants.push(FlockGrant {
                owner: owner.clone(),
                mode,
            });
            LockAttempt::Complete
        }
    }

    fn lock(
        &self,
        owner: &OpenedDescriptionCapability,
        lease: &OpenedDescriptionLease,
        mode: FlockMode,
        nonblocking: bool,
    ) -> FlockOutcome {
        loop {
            match self.attempt_lock(owner, lease, mode, nonblocking) {
                LockAttempt::Complete => return FlockOutcome::Complete,
                LockAttempt::Converted(old_grant) => {
                    // Conversion is deliberately non-atomic: publish removal
                    // before competing for the target mode. Dropping the old
                    // capability also stays outside the domain guard.
                    drop(old_grant);
                    self.recheck.publish(usize::MAX, false);
                },
                LockAttempt::Wait => {
                    let predicate_ready = self.recheck.listen(false, || {
                        let grants = self.grants.lock();
                        !lease.is_live() || !Self::conflicts(&grants, owner, mode)
                    });
                    if !predicate_ready {
                        // The old conversion mode, if any, was already removed
                        // and is never restored on signal interruption.
                        return FlockOutcome::Interrupted;
                    }
                },
                LockAttempt::WouldBlock => return FlockOutcome::WouldBlock,
                LockAttempt::Retired => return FlockOutcome::Retired,
            }
        }
    }

    fn unlock(
        &self,
        owner: &OpenedDescriptionCapability,
        lease: &OpenedDescriptionLease,
    ) -> FlockOutcome {
        let removed = {
            let mut grants = self.grants.lock();
            if !lease.is_live() {
                return FlockOutcome::Retired;
            }
            Self::owner_index(&grants, owner).map(|index| grants.swap_remove(index))
        };

        if let Some(grant) = removed {
            drop(grant);
            self.recheck.publish(usize::MAX, false);
        }
        FlockOutcome::Complete
    }

    fn retire(&self, same_owner: impl Fn(&OpenedDescriptionCapability) -> bool) {
        let removed = {
            let mut grants = self.grants.lock();
            let mut found = None;
            for (index, grant) in grants.iter().enumerate() {
                if same_owner(&grant.owner) {
                    assert!(found.is_none(), "retiring flock owner has duplicate grants");
                    found = Some(index);
                }
            }
            found.map(|index| grants.swap_remove(index))
        };

        // Retirement is also a liveness change for a waiter that never held a
        // grant. Always submit a recheck after detached grant destruction.
        drop(removed);
        self.recheck.publish(usize::MAX, false);
    }
}

pub(crate) fn request_flock(
    file: &File,
    owner: &OpenedDescriptionCapability,
    operation: FlockOperation,
) -> FlockOutcome {
    let Some(lease) = owner.try_lease() else {
        return FlockOutcome::Retired;
    };
    let domain = file.inode().flock_domain();

    match operation {
        FlockOperation::Lock { mode, nonblocking } => domain.lock(owner, &lease, mode, nonblocking),
        FlockOperation::Unlock => domain.unlock(owner, &lease),
    }
}

pub(crate) fn retire_flock(file: &File, same_owner: impl Fn(&OpenedDescriptionCapability) -> bool) {
    file.inode().flock_domain().retire(same_owner);
}
