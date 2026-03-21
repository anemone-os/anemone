//! Spin-based lock implementation.

use core::ops::{Deref, DerefMut};

use crate::prelude::*;

#[derive(Debug)]
pub struct SpinLock<T: ?Sized> {
    lock: spin::Mutex<T>,
}

#[derive(Debug)]
pub struct NoPreemptGuard<'a, T: ?Sized> {
    guard: spin::MutexGuard<'a, T>,
}

#[derive(Debug)]
pub struct IrqSaveGuard<'a, T: ?Sized> {
    guard: Option<spin::MutexGuard<'a, T>>,
    _intr_guard: IntrGuard,
}

impl<'a, T: ?Sized> Drop for IrqSaveGuard<'a, T> {
    fn drop(&mut self) {
        // First, drop the lock guard to release the lock.
        // This ensures that we don't hold the lock while restoring IRQ flags, which
        // could lead to deadlocks if an interrupt occurs while we still hold
        // the lock.
        _ = self.guard.take();

        // intr_guard will be dropped automatically after this.
    }
}

impl<T> SpinLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: spin::Mutex::new(data),
        }
    }
}

impl<T: ?Sized> SpinLock<T> {
    #[track_caller]
    pub fn lock(&self) -> NoPreemptGuard<'_, T> {
        todo!("implement scheduler first");
    }

    #[track_caller]
    pub fn lock_irqsave(&self) -> IrqSaveGuard<'_, T> {
        loop {
            let _intr_guard = IntrGuard::new();
            if let Some(guard) = self.lock.try_lock() {
                break IrqSaveGuard {
                    guard: Some(guard),
                    _intr_guard,
                };
            }
            _ = _intr_guard; // drop to restore interrupts before spinning
            core::hint::spin_loop();
        }
    }

    #[track_caller]
    pub fn try_lock_irqsave(&self) -> Option<IrqSaveGuard<'_, T>> {
        let _intr_guard = IntrGuard::new();
        let guard = self.lock.try_lock()?;
        Some(IrqSaveGuard {
            guard: Some(guard),
            _intr_guard,
        })
    }
}

impl<T: ?Sized> Deref for NoPreemptGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &*self.guard
    }
}

impl<T: ?Sized> DerefMut for NoPreemptGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.guard
    }
}

impl<T: ?Sized> Deref for IrqSaveGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard.as_ref().expect("lock should be held").deref()
    }
}

impl<T: ?Sized> DerefMut for IrqSaveGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard
            .as_mut()
            .expect("lock should be held")
            .deref_mut()
    }
}
