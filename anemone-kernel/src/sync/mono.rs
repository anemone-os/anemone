use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicBool, Ordering},
};

/// A container for data that relies on environment-guaranteed sequential
/// access.
///
/// `MonoFlow` is a highly-specialized synchronization primitive designed for
/// scenarios where data is accessed in a strictly sequential manner. Accessse
/// from multiple control flows can exist, but they must be fully **serialized**
/// and **non-overlapping** in time.
///
/// By ensuring no two control flows ever hold a reference to the inner data
/// simultaneously, `MonoFlow` can safely implement [`Sync`], allowing it to be
/// strored in global structures without any overhead of hardware
/// synchronization. I.e., this is a zero-cost abstraction.
///
/// # Safety
///
/// 1. **Sequential Access**: The core requirement for using `MonoFlow`.
/// 2. **Non-Reentrancy**: The requirement from Rust's aliasing rules. Currently
///    enforced in dev builds.
#[derive(Debug)]
pub struct MonoFlow<T> {
    data: UnsafeCell<T>,
    #[cfg(debug_assertions)]
    borrowed: AtomicBool,
}

unsafe impl<T> Sync for MonoFlow<T> {}

impl<T> MonoFlow<T> {
    /// Create a new [`MonoFlow`] wrapping the given data.
    ///
    /// # Safety
    ///
    /// See the safety requirements of the [`MonoFlow`] type.
    pub const unsafe fn new(data: T) -> Self
    where
        T: Sized,
    {
        MonoFlow {
            data: UnsafeCell::new(data),
            #[cfg(debug_assertions)]
            borrowed: AtomicBool::new(false),
        }
    }

    /// Access the inner data.
    #[inline(always)]
    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        #[cfg(debug_assertions)]
        {
            if self.borrowed.swap(true, Ordering::Acquire) {
                panic!("MonoFlow: data is already borrowed");
            }
        }

        let result = f(unsafe { &*self.data.get() });

        #[cfg(debug_assertions)]
        {
            self.borrowed.store(false, Ordering::Release);
        }

        result
    }

    /// Mutably access the inner data.
    #[inline(always)]
    pub fn with_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        #[cfg(debug_assertions)]
        {
            if self.borrowed.swap(true, Ordering::Acquire) {
                panic!("MonoFlow: data is already borrowed");
            }
        }

        let result = f(unsafe { &mut *self.data.get() });

        #[cfg(debug_assertions)]
        {
            self.borrowed.store(false, Ordering::Release);
        }

        result
    }
}

/// A container for data that is initialized only once.
///
/// See [`MonoFlow`] for details on the synchronization model.
///
/// **In effect, since [`MonoOnce`] does not provide mutable access to the inner
/// data, it can be simultaneously accessed by multiple control flows.**
///
/// **The 'Mono' in the name stands for the fact that the action to initialize
/// the inner data is guaranteed to be performed only once and before any access
/// to the inner data.**
#[derive(Debug)]
pub struct MonoOnce<T> {
    data: UnsafeCell<MaybeUninit<T>>,
    #[cfg(debug_assertions)]
    initialized: AtomicBool,
}

unsafe impl<T: Sync> Sync for MonoOnce<T> {}

impl<T> MonoOnce<T> {
    pub const unsafe fn new() -> Self {
        MonoOnce {
            data: UnsafeCell::new(MaybeUninit::uninit()),
            #[cfg(debug_assertions)]
            initialized: AtomicBool::new(false),
        }
    }

    /// Create a new [`MonoOnce`] with the inner data already initialized.
    ///
    /// This does not make any guarantee on the initialization of the inner
    /// data, and is only a convenience function for cases where the inner data
    /// can be initialized at compile time.
    pub const unsafe fn from_partial_initialized(init: T) -> Self {
        MonoOnce {
            data: UnsafeCell::new(MaybeUninit::new(init)),
            #[cfg(debug_assertions)]
            initialized: AtomicBool::new(false),
        }
    }

    pub fn init<F>(&self, init: F)
    where
        F: FnOnce(&mut MaybeUninit<T>),
        T: Sized,
    {
        #[cfg(debug_assertions)]
        {
            if !self.initialized.load(Ordering::Acquire) {
                init(unsafe { &mut *self.data.get() });
                self.initialized.store(true, Ordering::Release);
            } else {
                panic!("MonoOnce: already initialized");
            }
        }

        #[cfg(not(debug_assertions))]
        {
            init(unsafe { &mut *self.data.get() });
        }
    }

    pub fn get(&self) -> &T {
        unsafe { (&*self.data.get()).assume_init_ref() }
    }
}
