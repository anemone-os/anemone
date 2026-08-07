//! Thread-group timer ID namespace, create publication, and bulk teardown.

use super::{
    PosixTimerClock, PosixTimerNotification, PosixTimerSetting,
    timer::{NotificationKind, PosixTimer},
};
use crate::{
    prelude::*,
    task::sig::{PosixTimerSignalCallback, PosixTimerSignalRegistration, SigNo},
};

#[derive(Debug)]
pub struct PosixTimers {
    table: NoIrqSpinLock<PosixTimerTable>,
}

#[derive(Debug, Default)]
struct PosixTimerTable {
    slots: Vec<Option<PosixTimerSlot>>,
    next_reservation: u64,
}

#[derive(Debug)]
enum PosixTimerSlot {
    /// A create operation owns this numeric ID, but syscall lookup must still
    /// report EINVAL until user copyout succeeds and publication commits. The
    /// token prevents an old create transaction from publishing over an ID
    /// reused after exec teardown clears its reservation.
    Reserved(u64),
    Published(Arc<PosixTimer>),
}

impl PosixTimerTable {
    fn reserve(&mut self) -> Result<(i32, u64), SysError> {
        self.next_reservation = self
            .next_reservation
            .checked_add(1)
            .expect("POSIX timer reservation identity exhausted");
        let token = self.next_reservation;
        if let Some((index, slot)) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.is_none())
        {
            *slot = Some(PosixTimerSlot::Reserved(token));
            return Ok((i32::try_from(index).map_err(|_| SysError::Again)?, token));
        }

        let id = i32::try_from(self.slots.len()).map_err(|_| SysError::Again)?;
        self.slots
            .try_reserve(1)
            .map_err(|_| SysError::OutOfMemory)?;
        self.slots.push(Some(PosixTimerSlot::Reserved(token)));
        Ok((id, token))
    }

    fn release_reservation(&mut self, id: i32, token: u64) {
        let Some(slot) = self.slots.get_mut(id as usize) else {
            return;
        };
        if matches!(slot, Some(PosixTimerSlot::Reserved(current)) if *current == token) {
            *slot = None;
        }
    }

    fn publish(&mut self, id: i32, token: u64, timer: Arc<PosixTimer>) -> Result<(), SysError> {
        let Some(slot) = self.slots.get_mut(id as usize) else {
            return Err(SysError::NoSuchProcess);
        };
        if !matches!(slot, Some(PosixTimerSlot::Reserved(current)) if *current == token) {
            return Err(SysError::NoSuchProcess);
        }
        *slot = Some(PosixTimerSlot::Published(timer));
        Ok(())
    }

    fn get(&self, id: i32) -> Option<Arc<PosixTimer>> {
        if id < 0 {
            return None;
        }
        match self.slots.get(id as usize)?.as_ref()? {
            PosixTimerSlot::Reserved(_) => None,
            PosixTimerSlot::Published(timer) => Some(timer.clone()),
        }
    }

    fn remove(&mut self, id: i32) -> Option<Arc<PosixTimer>> {
        if id < 0 {
            return None;
        }
        match self.slots.get_mut(id as usize)?.take()? {
            PosixTimerSlot::Reserved(_) => None,
            PosixTimerSlot::Published(timer) => Some(timer),
        }
    }

    fn take_all(&mut self) -> Vec<Option<PosixTimerSlot>> {
        core::mem::take(&mut self.slots)
    }
}

impl PosixTimers {
    pub const fn new() -> Self {
        Self {
            table: NoIrqSpinLock::new(PosixTimerTable {
                slots: Vec::new(),
                next_reservation: 0,
            }),
        }
    }
}

impl Drop for PosixTimers {
    fn drop(&mut self) {
        let slots = self.table.lock().take_all();
        delete_slots(slots);
    }
}

/// Prepared create transaction. Drop rolls back both the unpublished ID and
/// any signal resource installed while preparing the object.
pub(crate) struct PreparedPosixTimer {
    owner: Weak<ThreadGroup>,
    id: i32,
    reservation: u64,
    timer: Option<Arc<PosixTimer>>,
}

impl PreparedPosixTimer {
    pub(crate) fn id(&self) -> i32 {
        self.id
    }

    pub(crate) fn publish(mut self) -> Result<(), SysError> {
        let owner = self.owner.upgrade().ok_or(SysError::NoSuchProcess)?;
        let timer = self
            .timer
            .as_ref()
            .expect("prepared POSIX timer was consumed")
            .clone();
        owner
            .posix_timers
            .table
            .lock()
            .publish(self.id, self.reservation, timer)?;
        self.timer.take();
        Ok(())
    }
}

impl Drop for PreparedPosixTimer {
    fn drop(&mut self) {
        if self.timer.is_none() {
            return;
        }
        if let Some(owner) = self.owner.upgrade() {
            owner
                .posix_timers
                .table
                .lock()
                .release_reservation(self.id, self.reservation);
        }
    }
}

impl ThreadGroup {
    pub(crate) fn prepare_posix_timer(
        self: &Arc<Self>,
        clock: PosixTimerClock,
        notification: PosixTimerNotification,
    ) -> Result<PreparedPosixTimer, SysError> {
        let (id, reservation) = self.posix_timers.table.lock().reserve()?;
        let kind = match &notification {
            PosixTimerNotification::None => NotificationKind::None,
            PosixTimerNotification::DefaultSignal | PosixTimerNotification::Signal { .. } => {
                NotificationKind::SharedSignal
            },
            PosixTimerNotification::ThreadSignal { .. } => NotificationKind::ThreadSignal,
        };
        let timer = Arc::new(PosixTimer::new(id, clock, kind));
        let registration = match notification {
            PosixTimerNotification::None => None,
            PosixTimerNotification::DefaultSignal => {
                let weak_timer = Arc::downgrade(&timer);
                let callback: Arc<PosixTimerSignalCallback> = Arc::new(move |identity, _reason| {
                    if let Some(timer) = weak_timer.upgrade() {
                        timer.signal_delivered(identity);
                    }
                    None
                });
                Some(PosixTimerSignalRegistration::try_new(
                    self,
                    SigNo::SIGALRM,
                    id,
                    id as u64,
                    callback,
                ))
            },
            PosixTimerNotification::Signal { no, sigval } => {
                let weak_timer = Arc::downgrade(&timer);
                let callback: Arc<PosixTimerSignalCallback> = Arc::new(move |identity, _reason| {
                    if let Some(timer) = weak_timer.upgrade() {
                        timer.signal_delivered(identity);
                    }
                    None
                });
                Some(PosixTimerSignalRegistration::try_new(
                    self, no, id, sigval, callback,
                ))
            },
            PosixTimerNotification::ThreadSignal { target, no, sigval } => {
                let weak_timer = Arc::downgrade(&timer);
                let callback: Arc<PosixTimerSignalCallback> = Arc::new(move |identity, reason| {
                    weak_timer
                        .upgrade()
                        .and_then(|timer| timer.thread_signal_completed(identity, reason))
                });
                Some(
                    PosixTimerSignalRegistration::try_new_private(
                        &target, no, id, sigval, callback,
                    )
                    .map_err(|error| match error {
                        SysError::NoSuchProcess => SysError::InvalidArgument,
                        other => other,
                    }),
                )
            },
        };
        if let Some(registration) = registration {
            match registration {
                Ok(registration) => {
                    timer.install_signal_registration(registration);
                },
                Err(error) => {
                    self.posix_timers
                        .table
                        .lock()
                        .release_reservation(id, reservation);
                    return Err(error);
                },
            }
        }
        Ok(PreparedPosixTimer {
            owner: Arc::downgrade(self),
            id,
            reservation,
            timer: Some(timer),
        })
    }

    fn get_posix_timer(&self, id: i32) -> Result<Arc<PosixTimer>, SysError> {
        self.posix_timers
            .table
            .lock()
            .get(id)
            .ok_or(SysError::InvalidArgument)
    }

    pub(crate) fn posix_timer_gettime(&self, id: i32) -> Result<PosixTimerSetting, SysError> {
        Ok(self.get_posix_timer(id)?.setting_snapshot())
    }

    pub(crate) fn posix_timer_settime(
        &self,
        id: i32,
        setting: PosixTimerSetting,
        absolute: bool,
    ) -> Result<PosixTimerSetting, SysError> {
        self.get_posix_timer(id)?.settime(setting, absolute)
    }

    pub(crate) fn posix_timer_getoverrun(&self, id: i32) -> Result<i32, SysError> {
        self.get_posix_timer(id)?.getoverrun()
    }

    pub(crate) fn delete_posix_timer(&self, id: i32) -> Result<(), SysError> {
        // ID removal is the syscall visibility linearization point. Object
        // generation and physical queue cancellation happen only afterwards.
        let timer = self
            .posix_timers
            .table
            .lock()
            .remove(id)
            .ok_or(SysError::InvalidArgument)?;
        timer.delete();
        Ok(())
    }

    pub(in crate::task) fn delete_all_posix_timers(&self) {
        let slots = self.posix_timers.table.lock().take_all();
        delete_slots(slots);
    }
}

fn delete_slots(slots: Vec<Option<PosixTimerSlot>>) {
    for slot in slots {
        if let Some(PosixTimerSlot::Published(timer)) = slot {
            timer.delete();
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn timer_id_reservation_is_invisible_and_reusable() {
        let mut table = PosixTimerTable::default();
        let (first, first_token) = table.reserve().unwrap();
        let (second, _) = table.reserve().unwrap();
        assert_eq!((first, second), (0, 1));
        assert!(table.get(first).is_none());
        table.release_reservation(first, first_token);
        assert_eq!(table.reserve().unwrap().0, first);
    }

    #[kunit]
    fn stale_reservation_cannot_release_or_publish_reused_id() {
        let mut table = PosixTimerTable::default();
        let (id, stale_token) = table.reserve().unwrap();
        table.take_all();
        let (reused_id, current_token) = table.reserve().unwrap();
        assert_eq!(reused_id, id);
        assert_ne!(current_token, stale_token);

        table.release_reservation(id, stale_token);
        assert!(matches!(
            table.slots[id as usize],
            Some(PosixTimerSlot::Reserved(token)) if token == current_token
        ));
        let stale_timer = Arc::new(PosixTimer::new(
            id,
            PosixTimerClock::Monotonic,
            NotificationKind::None,
        ));
        assert_eq!(
            table.publish(id, stale_token, stale_timer),
            Err(SysError::NoSuchProcess)
        );
    }
}
