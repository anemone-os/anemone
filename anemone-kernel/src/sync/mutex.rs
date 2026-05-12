use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use crate::{percpu::in_hwirq, prelude::*};

const NO_LOCKER_PTR: usize = 39;

#[derive(Debug)]
pub struct Mutex<T: ?Sized> {
    locked: AtomicBool,
    lock_released: Event,
    /// don't use [AtomicPtr]. this is only for identifying the locker.
    locker: AtomicUsize,
    data: UnsafeCell<T>,
}

impl<T> Mutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            lock_released: Event::new(),
            locker: AtomicUsize::new(NO_LOCKER_PTR),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T: ?Sized> Mutex<T> {
    #[track_caller]
    pub fn lock(&self) -> MutexGuard<'_, T> {
        assert!(!in_hwirq(), "Mutex cannot be locked in hwirq context");
        assert!(
            IntrArch::local_intr_enabled(),
            "Mutex cannot be locked when interrupts are disabled"
        );
        assert!(
            allow_preempt(),
            "Mutex cannot be locked when preemption is disabled"
        );
        assert!(
            self.locker.load(Ordering::Acquire) != Arc::as_ptr(&get_current_task()) as usize,
            "Mutex cannot be locked recursively"
        );

        // TODO: assert that current task is not holding any other mutex, otherwise
        // deadlock can occur. that no spinlock is held is already ensured by
        // the fact that preemption is allowed.

        // fast path: try to acquire the lock without sleeping.
        if let Ok(_) =
            self.locked
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        {
            let prev_ptr = self.locker.compare_exchange(
                NO_LOCKER_PTR,
                Arc::as_ptr(&get_current_task()) as usize,
                Ordering::SeqCst,
                Ordering::Relaxed,
            );
            assert!(
                prev_ptr.is_ok(),
                "Mutex locker should be invalid when lock is released"
            );
            return MutexGuard {
                mutex: self,
                _not_send: PhantomData,
            };
        }

        // slow path: wait until the lock is released.
        //
        // we don't need a loop here. since we use atomic compare_exchange.

        self.lock_released.listen_uninterruptible(true, || {
            self.locked
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        });

        let prev_ptr = self.locker.compare_exchange(
            NO_LOCKER_PTR,
            Arc::as_ptr(&get_current_task()) as usize,
            Ordering::SeqCst,
            Ordering::Relaxed,
        );
        assert!(
            prev_ptr.is_ok(),
            "Mutex locker should be invalid when lock is released"
        );

        MutexGuard {
            mutex: self,
            _not_send: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct MutexGuard<'a, T: ?Sized> {
    mutex: &'a Mutex<T>,
    _not_send: PhantomData<*mut ()>,
}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T: ?Sized> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        debug_assert!(self.mutex.locked.load(Ordering::Acquire));
        let current_tid = current_task_id().get();
        let prev_ptr = self.mutex.locker.compare_exchange(
            Arc::as_ptr(&get_current_task()) as usize,
            NO_LOCKER_PTR,
            Ordering::SeqCst,
            Ordering::Relaxed,
        );
        assert!(prev_ptr.is_ok(), "Mutex can only be unlocked by the locker");
        self.mutex.locked.store(false, Ordering::Release);

        self.mutex.lock_released.publish(1, true);
    }
}

unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}
