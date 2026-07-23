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
    generation: u64,
    entry: Option<RelationEntry>,
}

struct RelationRegistry {
    slots: SpinLock<Vec<RelationSlot>>,
}

pub(super) struct PreparedRelations {
    slots: Vec<RelationSlot>,
}

#[derive(Clone)]
pub(super) struct RelationSnapshot {
    endpoint: Arc<TtyEndpoint>,
    session: TtySession,
    foreground: Option<TtyProcessGroup>,
    generation: u64,
}

static RELATIONS: MonoOnce<RelationRegistry> = unsafe { MonoOnce::new() };

fn next_generation(generation: u64) -> u64 {
    generation
        .checked_add(1)
        .expect("TTY relation generation overflow")
}

pub(super) fn prepare(endpoints: &[Arc<TtyEndpoint>]) -> Result<PreparedRelations, SysError> {
    let mut slots = Vec::new();
    slots
        .try_reserve_exact(endpoints.len())
        .map_err(|_| SysError::OutOfMemory)?;
    for endpoint in endpoints {
        slots.push(RelationSlot {
            endpoint: endpoint.clone(),
            generation: 0,
            entry: None,
        });
    }
    Ok(PreparedRelations { slots })
}

pub(super) fn install(prepared: PreparedRelations) {
    RELATIONS.init(|slot| {
        slot.write(RelationRegistry {
            slots: SpinLock::new(prepared.slots),
        });
    });
}

fn registry() -> &'static RelationRegistry {
    RELATIONS.get()
}

fn raw_endpoint_snapshot(endpoint: &Arc<TtyEndpoint>) -> Option<RelationSnapshot> {
    let slots = registry().slots.lock();
    let slot = slots
        .iter()
        .find(|slot| Arc::ptr_eq(&slot.endpoint, endpoint))?;
    let entry = slot.entry.as_ref()?;
    Some(RelationSnapshot {
        endpoint: slot.endpoint.clone(),
        session: entry.session.clone(),
        foreground: entry.foreground.clone(),
        generation: slot.generation,
    })
}

fn raw_session_snapshot(session: &TtySession) -> Option<RelationSnapshot> {
    let slots = registry().slots.lock();
    let slot = slots.iter().find(|slot| {
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
        generation: slot.generation,
    })
}

fn remove_if(snapshot: &RelationSnapshot) -> Option<RelationEntry> {
    let mut slots = registry().slots.lock();
    let slot = slots
        .iter_mut()
        .find(|slot| Arc::ptr_eq(&slot.endpoint, &snapshot.endpoint))?;
    if slot.generation != snapshot.generation
        || !slot
            .entry
            .as_ref()
            .is_some_and(|entry| entry.session.same_identity(&snapshot.session))
    {
        return None;
    }
    slot.generation = next_generation(slot.generation);
    slot.entry.take()
}

fn clear_stale_foreground(snapshot: &RelationSnapshot) -> Option<TtyProcessGroup> {
    let mut slots = registry().slots.lock();
    let slot = slots
        .iter_mut()
        .find(|slot| Arc::ptr_eq(&slot.endpoint, &snapshot.endpoint))?;
    if slot.generation != snapshot.generation
        || !slot
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
    slot.generation = next_generation(slot.generation);
    entry.foreground.take()
}

fn validate_snapshot(snapshot: RelationSnapshot) -> Option<RelationSnapshot> {
    if !snapshot.session.is_live() {
        let removed = remove_if(&snapshot);
        if removed.is_some() {
            knoticeln!(
                "TTY: lazily detached stale session {} generation {}",
                snapshot.session.sid(),
                snapshot.generation
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
        current.generation == self.generation
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
            let slots = registry().slots.lock();
            let mut endpoint_slot = None;
            let mut conflict = None;
            for slot in slots.iter() {
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
                            generation: slot.generation,
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
            let _ = endpoint_slot.expect("unpublished endpoint used for TTY relation");
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
            let mut slots = registry().slots.lock();
            if slots.iter().any(|slot| {
                slot.entry.as_ref().is_some_and(|entry| {
                    entry.session.same_identity(caller.session())
                        || Arc::ptr_eq(&slot.endpoint, endpoint)
                })
            }) {
                None
            } else {
                let slot = slots
                    .iter_mut()
                    .find(|slot| Arc::ptr_eq(&slot.endpoint, endpoint))
                    .expect("unpublished endpoint used for TTY relation");
                slot.generation = next_generation(slot.generation);
                slot.entry = Some(RelationEntry {
                    session: caller.session().clone(),
                    foreground: Some(caller.process_group().clone()),
                });
                Some(slot.generation)
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
        let mut slots = registry().slots.lock();
        let Some(slot) = slots
            .iter_mut()
            .find(|slot| Arc::ptr_eq(&slot.endpoint, &snapshot.endpoint))
        else {
            return false;
        };
        if slot.generation != snapshot.generation
            || !slot
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
        slot.generation = next_generation(slot.generation);
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
            snapshot.generation
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
            snapshot.generation
        );
        drop(removed);
        return;
    }
}
