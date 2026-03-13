use core::ops::{Deref, DerefMut};

use crate::prelude::*;

#[derive(Debug)]
pub struct RwLock<T: ?Sized> {
    lock: spin::RwLock<T>,
}

#[derive(Debug)]
pub struct ReadIrqSaveGuard<'a, T: ?Sized> {
    guard: Option<spin::RwLockReadGuard<'a, T>>,
    _intr_guard: IntrGuard,
}

#[derive(Debug)]
pub struct WriteIrqSaveGuard<'a, T: ?Sized> {
    guard: Option<spin::RwLockWriteGuard<'a, T>>,
    _intr_guard: IntrGuard,
}

impl<'a, T: ?Sized> Drop for ReadIrqSaveGuard<'a, T> {
    fn drop(&mut self) {
        _ = self.guard.take();
    }
}

impl<'a, T: ?Sized> Drop for WriteIrqSaveGuard<'a, T> {
    fn drop(&mut self) {
        _ = self.guard.take();
    }
}

impl<T> RwLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: spin::RwLock::new(data),
        }
    }
}

impl<T: ?Sized> RwLock<T> {
    #[track_caller]
    pub fn read(&self) -> spin::RwLockReadGuard<'_, T> {
        todo!("implement scheduler first");
    }

    #[track_caller]
    pub fn write(&self) -> spin::RwLockWriteGuard<'_, T> {
        todo!("implement scheduler first");
    }

    #[track_caller]
    pub fn read_irqsave(&self) -> ReadIrqSaveGuard<'_, T> {
        loop {
            let _intr_guard = IntrGuard::new();
            if let Some(guard) = self.lock.try_read() {
                break ReadIrqSaveGuard {
                    guard: Some(guard),
                    _intr_guard,
                };
            }
            _ = _intr_guard;
            core::hint::spin_loop();
        }
    }

    #[track_caller]
    pub fn write_irqsave(&self) -> WriteIrqSaveGuard<'_, T> {
        loop {
            let _intr_guard = IntrGuard::new();
            if let Some(guard) = self.lock.try_write() {
                break WriteIrqSaveGuard {
                    guard: Some(guard),
                    _intr_guard,
                };
            }
            _ = _intr_guard;
            core::hint::spin_loop();
        }
    }
}

impl<T: ?Sized> Deref for ReadIrqSaveGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard.as_ref().expect("lock should be held").deref()
    }
}

impl<T: ?Sized> Deref for WriteIrqSaveGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard.as_ref().expect("lock should be held").deref()
    }
}

impl<T: ?Sized> DerefMut for WriteIrqSaveGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard
            .as_mut()
            .expect("lock should be held")
            .deref_mut()
    }
}
