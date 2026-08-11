//! Controlling-terminal relation owner.

use crate::{
    prelude::*,
    task::jobctl::{TtyCaller, TtyProcessGroup, TtySession, TtySessionLeader, TtyTerminalSignal},
};

use super::TtyEndpoint;

struct RelationEntry {
    session: TtySession,
    foreground: Option<TtyProcessGroup>,
}

struct RelationSlot {
    endpoint: Arc<TtyEndpoint>,
    participant_generation: u64,
    relation_generation: u64,
    entry: Option<RelationEntry>,
}

impl RelationSlot {
    fn matches_version(
        &self,
        endpoint: &Arc<TtyEndpoint>,
        participant_generation: u64,
        relation_generation: u64,
    ) -> bool {
        Arc::ptr_eq(&self.endpoint, endpoint)
            && self.participant_generation == participant_generation
            && self.relation_generation == relation_generation
    }
}

struct RelationRegistry {
    inner: SpinLock<RelationRegistryInner>,
}

struct RelationRegistryInner {
    slots: Vec<RelationSlot>,
    next_participant_generation: u64,
}

/// One-shot authority created with a semantic endpoint and consumed before
/// that endpoint becomes visible. It carries no registry liveness truth.
pub(super) struct RelationEnrollment {
    endpoint: Arc<TtyEndpoint>,
}

/// Exact retirement capability for one committed registry participation.
/// Registry membership remains authoritative; this key is only a stable
/// operation capability and may be stale after an earlier retirement.
pub(super) struct RelationParticipant {
    key: ParticipantKey,
}

struct ParticipantKey {
    endpoint: Arc<TtyEndpoint>,
    generation: u64,
}

#[derive(Clone)]
pub(super) struct RelationSnapshot {
    endpoint: Arc<TtyEndpoint>,
    session: TtySession,
    foreground: Option<TtyProcessGroup>,
    participant_generation: u64,
    relation_generation: u64,
}

static RELATIONS: Lazy<RelationRegistry> = Lazy::new(RelationRegistry::new);

fn next_generation(generation: u64) -> u64 {
    generation
        .checked_add(1)
        .expect("TTY relation generation overflow")
}

impl RelationRegistry {
    fn new() -> Self {
        Self {
            inner: SpinLock::new(RelationRegistryInner {
                slots: Vec::new(),
                next_participant_generation: 1,
            }),
        }
    }

    fn enroll(&self, endpoint: Arc<TtyEndpoint>) -> Result<ParticipantKey, SysError> {
        let generation = {
            let mut inner = self.inner.lock();
            if inner
                .slots
                .iter()
                .any(|slot| Arc::ptr_eq(&slot.endpoint, &endpoint))
            {
                return Err(SysError::DevAlreadyRegistered);
            }
            inner
                .slots
                .try_reserve(1)
                .map_err(|_| SysError::OutOfMemory)?;
            let generation = inner.next_participant_generation;
            inner.next_participant_generation = next_generation(generation);
            inner.slots.push(RelationSlot {
                endpoint: endpoint.clone(),
                participant_generation: generation,
                relation_generation: 0,
                entry: None,
            });
            generation
        };
        Ok(ParticipantKey {
            endpoint,
            generation,
        })
    }

    fn retire(&self, key: &ParticipantKey) -> bool {
        let removed = {
            let mut inner = self.inner.lock();
            let Some(index) = inner.slots.iter().position(|slot| {
                slot.participant_generation == key.generation
                    && Arc::ptr_eq(&slot.endpoint, &key.endpoint)
            }) else {
                return false;
            };
            inner.slots.remove(index)
        };
        // Endpoint, session, and foreground capabilities may run non-trivial
        // destructors. The registry is already undiscoverable before they drop.
        drop(removed);
        true
    }
}

impl RelationEnrollment {
    pub(super) fn new(endpoint: Arc<TtyEndpoint>) -> Self {
        Self { endpoint }
    }

    pub(super) fn commit(self) -> Result<RelationParticipant, SysError> {
        let key = registry().enroll(self.endpoint)?;
        Ok(RelationParticipant { key })
    }
}

impl RelationParticipant {
    pub(super) fn retire(&self) -> bool {
        registry().retire(&self.key)
    }
}

impl Drop for RelationParticipant {
    fn drop(&mut self) {
        let _ = self.retire();
    }
}

fn registry() -> &'static RelationRegistry {
    &RELATIONS
}

fn raw_endpoint_snapshot(endpoint: &Arc<TtyEndpoint>) -> Option<RelationSnapshot> {
    let inner = registry().inner.lock();
    let slot = inner
        .slots
        .iter()
        .find(|slot| Arc::ptr_eq(&slot.endpoint, endpoint))?;
    let entry = slot.entry.as_ref()?;
    Some(RelationSnapshot {
        endpoint: slot.endpoint.clone(),
        session: entry.session.clone(),
        foreground: entry.foreground.clone(),
        participant_generation: slot.participant_generation,
        relation_generation: slot.relation_generation,
    })
}

fn raw_session_snapshot(session: &TtySession) -> Option<RelationSnapshot> {
    let inner = registry().inner.lock();
    let slot = inner.slots.iter().find(|slot| {
        slot.entry
            .as_ref()
            .is_some_and(|entry| entry.session.same_identity(session))
    })?;
    let entry = slot
        .entry
        .as_ref()
        .expect("matched TTY relation disappeared");
    Some(RelationSnapshot {
        endpoint: slot.endpoint.clone(),
        session: entry.session.clone(),
        foreground: entry.foreground.clone(),
        participant_generation: slot.participant_generation,
        relation_generation: slot.relation_generation,
    })
}

fn remove_if(snapshot: &RelationSnapshot) -> Option<RelationEntry> {
    let mut inner = registry().inner.lock();
    let slot = inner
        .slots
        .iter_mut()
        .find(|slot| Arc::ptr_eq(&slot.endpoint, &snapshot.endpoint))?;
    if !slot.matches_version(
        &snapshot.endpoint,
        snapshot.participant_generation,
        snapshot.relation_generation,
    ) || !slot
        .entry
        .as_ref()
        .is_some_and(|entry| entry.session.same_identity(&snapshot.session))
    {
        return None;
    }
    slot.relation_generation = next_generation(slot.relation_generation);
    slot.entry.take()
}

fn clear_stale_foreground(snapshot: &RelationSnapshot) -> Option<TtyProcessGroup> {
    let mut inner = registry().inner.lock();
    let slot = inner
        .slots
        .iter_mut()
        .find(|slot| Arc::ptr_eq(&slot.endpoint, &snapshot.endpoint))?;
    if !slot.matches_version(
        &snapshot.endpoint,
        snapshot.participant_generation,
        snapshot.relation_generation,
    ) || !slot
        .entry
        .as_ref()
        .is_some_and(|entry| entry.session.same_identity(&snapshot.session))
    {
        return None;
    }
    let entry = slot
        .entry
        .as_mut()
        .expect("matched TTY relation disappeared");
    slot.relation_generation = next_generation(slot.relation_generation);
    entry.foreground.take()
}
fn validate_snapshot(snapshot: RelationSnapshot) -> Option<RelationSnapshot> {
    if !snapshot.session.is_live() {
        let removed = remove_if(&snapshot);
        if removed.is_some() {
            knoticeln!(
                "TTY: lazily detached stale session {} generation {}",
                snapshot.session.sid(),
                snapshot.relation_generation
            );
        }
        drop(removed);
        return None;
    }
    if snapshot
        .foreground
        .as_ref()
        .is_some_and(|foreground| !foreground.is_live_in(&snapshot.session))
    {
        let removed = clear_stale_foreground(&snapshot);
        drop(removed);
        return None;
    }
    Some(snapshot)
}

pub(super) fn endpoint_snapshot(endpoint: &Arc<TtyEndpoint>) -> Option<RelationSnapshot> {
    loop {
        let snapshot = raw_endpoint_snapshot(endpoint)?;
        if let Some(snapshot) = validate_snapshot(snapshot) {
            return Some(snapshot);
        }
    }
}

pub(super) fn signal_foreground(endpoint: &Arc<TtyEndpoint>, signal: TtyTerminalSignal) -> bool {
    let Some(snapshot) = endpoint_snapshot(endpoint) else {
        return false;
    };
    let Some(foreground) = snapshot.foreground() else {
        return false;
    };
    // The relation guard was released when the snapshot was formed. The task
    // owner revalidates both stable identities before broadcasting; a
    // concurrent foreground replacement may therefore linearize this effect
    // to the last legal relation state observed by the worker.
    foreground.signal_terminal(snapshot.session(), signal)
}

pub(super) fn current_endpoint(caller: &TtyCaller) -> Result<Arc<TtyEndpoint>, SysError> {
    loop {
        if !caller.revalidate() {
            return Err(SysError::NoSuchDeviceOrAddress);
        }
        let Some(snapshot) = raw_session_snapshot(caller.session()) else {
            return Err(SysError::NoSuchDeviceOrAddress);
        };
        if let Some(snapshot) = validate_snapshot(snapshot) {
            return Ok(snapshot.endpoint);
        }
    }
}

impl RelationSnapshot {
    pub(super) fn session(&self) -> &TtySession {
        &self.session
    }

    pub(super) fn foreground(&self) -> Option<&TtyProcessGroup> {
        self.foreground.as_ref()
    }

    pub(super) fn is_current(&self) -> bool {
        let Some(current) = endpoint_snapshot(&self.endpoint) else {
            return false;
        };
        current.participant_generation == self.participant_generation
            && current.relation_generation == self.relation_generation
            && current.session.same_identity(&self.session)
            && match (&current.foreground, &self.foreground) {
                (Some(current), Some(snapshot)) => current.same_identity(snapshot),
                (None, None) => true,
                _ => false,
            }
    }
}

pub(super) fn acquire(
    endpoint: &Arc<TtyEndpoint>,
    caller: &TtyCaller,
    readable: bool,
) -> Result<(), SysError> {
    if !caller.is_session_leader() || !caller.revalidate() {
        return Err(SysError::PermissionDenied);
    }

    loop {
        enum Inspection {
            Idempotent,
            Empty,
            Conflict(RelationSnapshot),
        }

        let inspection = {
            let inner = registry().inner.lock();
            let mut endpoint_slot = None;
            let mut conflict = None;
            for slot in inner.slots.iter() {
                if Arc::ptr_eq(&slot.endpoint, endpoint) {
                    endpoint_slot = Some(slot);
                }
                if let Some(entry) = &slot.entry {
                    if entry.session.same_identity(caller.session())
                        || Arc::ptr_eq(&slot.endpoint, endpoint)
                    {
                        let snapshot = RelationSnapshot {
                            endpoint: slot.endpoint.clone(),
                            session: entry.session.clone(),
                            foreground: entry.foreground.clone(),
                            participant_generation: slot.participant_generation,
                            relation_generation: slot.relation_generation,
                        };
                        if entry.session.same_identity(caller.session())
                            && Arc::ptr_eq(&slot.endpoint, endpoint)
                        {
                            conflict = Some(Inspection::Idempotent);
                            break;
                        }
                        conflict = Some(Inspection::Conflict(snapshot));
                    }
                }
            }
            if endpoint_slot.is_none() {
                return Err(SysError::UnsupportedIoctl);
            }
            conflict.unwrap_or(Inspection::Empty)
        };

        match inspection {
            Inspection::Idempotent => return Ok(()),
            Inspection::Conflict(snapshot) => {
                if snapshot.session.is_live() {
                    return Err(SysError::PermissionDenied);
                }
                let removed = remove_if(&snapshot);
                drop(removed);
                continue;
            },
            Inspection::Empty => {},
        }

        // The exact same relation returned above before this first-acquire file
        // access check. This preserves Linux idempotence without allowing a
        // write-only file to establish new controlling authority.
        if !readable || !caller.revalidate() {
            return Err(SysError::PermissionDenied);
        }

        let committed_generation = {
            let mut inner = registry().inner.lock();
            if inner.slots.iter().any(|slot| {
                slot.entry.as_ref().is_some_and(|entry| {
                    entry.session.same_identity(caller.session())
                        || Arc::ptr_eq(&slot.endpoint, endpoint)
                })
            }) {
                None
            } else {
                let Some(slot) = inner
                    .slots
                    .iter_mut()
                    .find(|slot| Arc::ptr_eq(&slot.endpoint, endpoint))
                else {
                    return Err(SysError::UnsupportedIoctl);
                };
                slot.relation_generation = next_generation(slot.relation_generation);
                slot.entry = Some(RelationEntry {
                    session: caller.session().clone(),
                    foreground: Some(caller.process_group().clone()),
                });
                Some(slot.relation_generation)
            }
        };
        if let Some(generation) = committed_generation {
            kinfoln!(
                "TTY: acquired controlling relation sid={} pgid={} generation={}",
                caller.session().sid(),
                caller.process_group().pgid(),
                generation
            );
            return Ok(());
        }
    }
}

pub(super) fn commit_foreground(snapshot: &RelationSnapshot, foreground: TtyProcessGroup) -> bool {
    let old = {
        let mut inner = registry().inner.lock();
        let Some(slot) = inner
            .slots
            .iter_mut()
            .find(|slot| Arc::ptr_eq(&slot.endpoint, &snapshot.endpoint))
        else {
            return false;
        };
        if !slot.matches_version(
            &snapshot.endpoint,
            snapshot.participant_generation,
            snapshot.relation_generation,
        ) || !slot
            .entry
            .as_ref()
            .is_some_and(|entry| entry.session.same_identity(&snapshot.session))
        {
            return false;
        }
        let entry = slot
            .entry
            .as_mut()
            .expect("matched TTY relation disappeared");
        slot.relation_generation = next_generation(slot.relation_generation);
        entry.foreground.replace(foreground)
    };
    drop(old);
    true
}

pub(super) fn detach(endpoint: &Arc<TtyEndpoint>, caller: &TtyCaller) -> Result<(), SysError> {
    loop {
        let snapshot = endpoint_snapshot(endpoint).ok_or(SysError::UnsupportedIoctl)?;
        if !snapshot.session.same_identity(caller.session()) {
            return Err(SysError::UnsupportedIoctl);
        }
        if !caller.is_session_leader() {
            // The first-version ABI deliberately rejects non-leader detach; it
            // does not silently turn TIOCNOTTY into a process-local no-op.
            knoticeln!(
                "TTY: rejecting non-leader TIOCNOTTY sid={}",
                caller.session().sid()
            );
            return Err(SysError::PermissionDenied);
        }
        let Some(removed) = remove_if(&snapshot) else {
            // A concurrent foreground replacement may only advance the
            // generation. Re-snapshot so detach cannot be lost to that race.
            continue;
        };
        kinfoln!(
            "TTY: explicitly detached sid={} generation={}",
            snapshot.session.sid(),
            snapshot.relation_generation
        );
        drop(removed);
        return Ok(());
    }
}

pub(crate) fn detach_exiting_session(leader: TtySessionLeader) {
    let session = leader.session();
    loop {
        let Some(snapshot) = raw_session_snapshot(session) else {
            return;
        };
        let Some(removed) = remove_if(&snapshot) else {
            // Foreground mutation can advance the relation generation between
            // snapshot and removal. Exit cleanup must retry that mismatch.
            continue;
        };
        kinfoln!(
            "TTY: exit detached sid={} generation={}",
            snapshot.session.sid(),
            snapshot.relation_generation
        );
        drop(removed);
        return;
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::{super::terminal::Terminal, *};
    use crate::device::tty::{TtyLineSnapshot, TtyParity};

    fn endpoint() -> Arc<TtyEndpoint> {
        Arc::new(TtyEndpoint {
            terminal: Terminal::try_new(TtyLineSnapshot {
                baud: 115200,
                parity: TtyParity::None,
                data_bits: 8,
            })
            .unwrap(),
            wake_source: Weak::new(),
        })
    }

    fn contains(key: &ParticipantKey) -> bool {
        registry().inner.lock().slots.iter().any(|slot| {
            slot.participant_generation == key.generation
                && Arc::ptr_eq(&slot.endpoint, &key.endpoint)
        })
    }

    fn commit_batch(
        enrollments: Vec<RelationEnrollment>,
    ) -> Result<Vec<RelationParticipant>, SysError> {
        let mut participants = Vec::new();
        participants
            .try_reserve_exact(enrollments.len())
            .map_err(|_| SysError::OutOfMemory)?;
        for enrollment in enrollments {
            participants.push(enrollment.commit()?);
        }
        Ok(participants)
    }

    #[kunit]
    fn duplicate_enrollment_fails_without_changing_membership() {
        let endpoint = endpoint();
        let participant = RelationEnrollment::new(endpoint.clone()).commit().unwrap();
        let next_generation = registry().inner.lock().next_participant_generation;

        assert_eq!(
            RelationEnrollment::new(endpoint).commit().err(),
            Some(SysError::DevAlreadyRegistered)
        );
        assert!(contains(&participant.key));
        assert_eq!(
            registry().inner.lock().next_participant_generation,
            next_generation
        );
    }

    #[kunit]
    fn failed_prepare_rolls_back_committed_participants() {
        let endpoint = endpoint();
        let weak = Arc::downgrade(&endpoint);
        let enrollments = vec![
            RelationEnrollment::new(endpoint.clone()),
            RelationEnrollment::new(endpoint.clone()),
        ];
        drop(endpoint);

        assert_eq!(
            commit_batch(enrollments).err(),
            Some(SysError::DevAlreadyRegistered)
        );
        assert!(weak.upgrade().is_none());
    }

    #[kunit]
    fn retirement_is_exact_and_idempotent() {
        let participant = RelationEnrollment::new(endpoint()).commit().unwrap();
        assert!(participant.retire());
        assert!(!participant.retire());
        assert!(!contains(&participant.key));
    }

    #[kunit]
    fn stale_participant_cleanup_cannot_hit_reenrollment() {
        let old_endpoint = endpoint();
        let old = RelationEnrollment::new(old_endpoint).commit().unwrap();
        assert!(old.retire());

        let new_endpoint = endpoint();
        assert!(!Arc::ptr_eq(&old.key.endpoint, &new_endpoint));
        let new = RelationEnrollment::new(new_endpoint).commit().unwrap();
        assert_ne!(old.key.generation, new.key.generation);
        drop(old);
        assert!(contains(&new.key));

        assert!(new.retire());
    }
}
