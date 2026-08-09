use crate::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::fs) struct DentryResidencyTicket(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct DentryIdentity(usize);

impl DentryIdentity {
    fn of(dentry: &Arc<Dentry>) -> Self {
        Self(Arc::as_ptr(dentry) as usize)
    }
}

/// Superblock-local owner of bounded extra positive-dentry residency.
///
/// The backend namespace and the parent weak-child map remain the namespace
/// truth. `residents` is the sole membership truth here; `fifo` only records
/// replacement order for identities whose `Arc` is owned by `residents`.
pub(in crate::fs) struct PositiveDentryResidency {
    generation: u64,
    capacity: usize,
    residents: HashMap<DentryIdentity, Arc<Dentry>>,
    fifo: VecDeque<DentryIdentity>,
}

/// Detached residency references whose destruction is deliberately deferred
/// until after the owner lock is released.
pub(in crate::fs) struct RetiredDentries(HashMap<DentryIdentity, Arc<Dentry>>);

impl Drop for RetiredDentries {
    fn drop(&mut self) {
        self.0.clear();
    }
}

impl PositiveDentryResidency {
    pub(in crate::fs) fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "positive dentry residency must be bounded");
        Self {
            generation: 0,
            capacity,
            residents: HashMap::new(),
            fifo: VecDeque::new(),
        }
    }

    pub(in crate::fs) fn ticket(&self) -> DentryResidencyTicket {
        DentryResidencyTicket(self.generation)
    }

    /// Invalidate admissions that began before a committed namespace change.
    pub(in crate::fs) fn invalidate(&mut self) {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("positive dentry residency generation overflow");
    }

    /// Retain `dentry` if its materialization is still fresh.
    ///
    /// Allocation failure is a cache miss: both containers reserve before any
    /// membership change. The returned eviction victim must be dropped after
    /// the residency lock is released because `Dentry::drop` enters the parent
    /// weak-child map.
    pub(in crate::fs) fn admit(
        &mut self,
        ticket: DentryResidencyTicket,
        dentry: &Arc<Dentry>,
    ) -> Option<Arc<Dentry>> {
        if ticket.0 != self.generation {
            return None;
        }

        let identity = DentryIdentity::of(dentry);
        if self.residents.contains_key(&identity) {
            return None;
        }

        let evicted = if self.residents.len() == self.capacity {
            let oldest = self
                .fifo
                .pop_front()
                .expect("residency membership and FIFO must have equal length");
            Some(
                self.residents
                    .remove(&oldest)
                    .expect("FIFO identity must name a resident dentry"),
            )
        } else {
            if self.residents.try_reserve(1).is_err() || self.fifo.try_reserve(1).is_err() {
                return None;
            }
            None
        };

        let previous = self.residents.insert(identity, dentry.clone());
        assert!(
            previous.is_none(),
            "dentry residency identity must be unique"
        );
        self.fifo.push_back(identity);
        self.assert_shape();
        evicted
    }

    /// Forget one unpublished dentry and return its strong reference.
    pub(in crate::fs) fn forget(&mut self, dentry: &Arc<Dentry>) -> Option<Arc<Dentry>> {
        let identity = DentryIdentity::of(dentry);
        if !self.residents.contains_key(&identity) {
            return None;
        }

        let position = self
            .fifo
            .iter()
            .position(|candidate| *candidate == identity)
            .expect("resident dentry must have FIFO replacement metadata");
        let removed_identity = self
            .fifo
            .remove(position)
            .expect("identified FIFO position must exist");
        assert_eq!(removed_identity, identity);

        let resident = self
            .residents
            .remove(&identity)
            .expect("checked resident dentry must remain present");
        assert!(Arc::ptr_eq(&resident, dentry));
        self.assert_shape();
        Some(resident)
    }

    /// Invalidate in-flight admissions and detach all residency references.
    pub(in crate::fs) fn drain(&mut self) -> RetiredDentries {
        self.invalidate();
        self.fifo.clear();
        let residents = core::mem::take(&mut self.residents);
        self.assert_shape();
        RetiredDentries(residents)
    }

    fn assert_shape(&self) {
        assert_eq!(
            self.residents.len(),
            self.fifo.len(),
            "dentry residency membership and replacement order diverged"
        );
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn detached_dentry(name: &str) -> Arc<Dentry> {
        let root = root_pathref();
        Arc::new(Dentry::new(
            name.to_string(),
            Some(root.dentry().clone()),
            root.inode().clone(),
        ))
    }

    #[kunit]
    fn admission_retains_and_fifo_evicts_at_capacity() {
        let mut residency = PositiveDentryResidency::new(1);

        let first = detached_dentry("kunit-residency-first");
        let first_weak = Arc::downgrade(&first);
        let first_ticket = residency.ticket();
        assert!(residency.admit(first_ticket, &first).is_none());
        drop(first);
        assert!(first_weak.upgrade().is_some());

        let second = detached_dentry("kunit-residency-second");
        let second_weak = Arc::downgrade(&second);
        let second_ticket = residency.ticket();
        let evicted = residency
            .admit(second_ticket, &second)
            .expect("a full FIFO must evict its oldest resident");
        drop(second);
        drop(evicted);

        assert!(first_weak.upgrade().is_none());
        assert!(second_weak.upgrade().is_some());
    }

    #[kunit]
    fn stale_admission_cannot_create_residency_and_forget_releases_current() {
        let mut residency = PositiveDentryResidency::new(2);
        let stale_ticket = residency.ticket();
        residency.invalidate();

        let stale = detached_dentry("kunit-residency-stale");
        let stale_weak = Arc::downgrade(&stale);
        assert!(residency.admit(stale_ticket, &stale).is_none());
        drop(stale);
        assert!(stale_weak.upgrade().is_none());

        let current = detached_dentry("kunit-residency-current");
        let current_weak = Arc::downgrade(&current);
        let current_ticket = residency.ticket();
        assert!(residency.admit(current_ticket, &current).is_none());
        drop(current);
        assert!(current_weak.upgrade().is_some());

        let current = current_weak
            .upgrade()
            .expect("current dentry must be resident");
        let forgotten = residency
            .forget(&current)
            .expect("current resident must be forgettable");
        drop(current);
        drop(forgotten);
        assert!(current_weak.upgrade().is_none());
    }

    #[kunit]
    fn drain_releases_all_residents_and_invalidates_prior_tickets() {
        let mut residency = PositiveDentryResidency::new(2);
        let prior_ticket = residency.ticket();

        let first = detached_dentry("kunit-residency-drain-first");
        let second = detached_dentry("kunit-residency-drain-second");
        let first_weak = Arc::downgrade(&first);
        let second_weak = Arc::downgrade(&second);
        assert!(residency.admit(prior_ticket, &first).is_none());
        assert!(residency.admit(prior_ticket, &second).is_none());
        drop(first);
        drop(second);

        let drained = residency.drain();
        drop(drained);
        assert!(first_weak.upgrade().is_none());
        assert!(second_weak.upgrade().is_none());

        let late = detached_dentry("kunit-residency-drain-late");
        let late_weak = Arc::downgrade(&late);
        assert!(residency.admit(prior_ticket, &late).is_none());
        drop(late);
        assert!(late_weak.upgrade().is_none());
    }
}
