//! Futex-related system calls.
//!
//! References:
//! - https://www.man7.org/linux/man-pages/man2/set_robust_list.2.html
//! - https://www.man7.org/linux/man-pages/man2/futex.2.html
//! - https://dept-info.labri.fr/~denis/Enseignement/2008-IR/Articles/01-futex.pdf

pub mod futex;
pub mod get_robust_list;
pub mod set_robust_list;

use anemone_abi::process::linux::futex::{
    FUTEX_OWNER_DIED, FUTEX_TID_MASK, FUTEX_WAITERS, RobustList, RobustListHead,
};
use hashbrown::hash_map::Entry;

use crate::{
    mm::uspace::{vma::ForkPolicy, vmo::VmObject},
    prelude::*,
    syscall::user_access::{UserReadPtr, user_addr},
    utils::either::Either,
};

/// [Weak] or [Arc] can't be used as key of a hash map since allocator might
/// reuse the same address after deallocation. So we use a monotonic counter.
macro_rules! gen_alloc_func {
    ($name:ident) => {
        paste::paste! {
            fn [<alloc_ $name _id>]() -> u64 {
                static COUNTER: AtomicU64 = AtomicU64::new(1);

                // simple cas
                loop {
                    let id = COUNTER.load(Ordering::Acquire);
                    if COUNTER
                        .compare_exchange(id, id + 1, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        return id;
                    }
                }
            }
        }
    };
}

gen_alloc_func!(uspace);
gen_alloc_func!(shared_object);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PrivateFutexKey {
    uspace_id: u64,
    /// Index in the user space.
    word_idx: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SharedFutexKey {
    so_id: u64,
    /// Index in the shared object.
    word_idx: usize,
}

type FutexKey = Either<PrivateFutexKey, SharedFutexKey>;

#[derive(Debug)]
struct FutexWaiter {
    /// Key of this waiter. This might change due to FUTEX_REQUEUE operation.
    key: SpinLock<FutexKey>,
    task: Arc<Task>,
    /// Each waiter block-listen on its own event.
    futex_available: Event,
    /// Whether this waker is already woken up by a futex wake operation.
    woken: AtomicBool,
    bitset: u32,
}

#[derive(Debug)]
struct Futex {
    waiters: VecDeque<Arc<FutexWaiter>>,
}

/// Global singleton. Managing all tasks' futexes.
///
/// TODO: bucketize.
struct FutexSet {
    /// <pointer of user space, (stable id, weak reference to original user
    /// space handle)>.
    private_registry: HashMap<u64, (u64, Weak<UserSpaceHandle>)>,
    private_futexes: HashMap<PrivateFutexKey, Futex>,
    /// <pointer of shared object, (stable id, weak reference to original shared
    /// object)>.
    shared_registry: HashMap<u64, (u64, Weak<dyn VmObject>)>,
    shared_futexes: HashMap<SharedFutexKey, Futex>,
}

impl FutexSet {
    fn create_futex(&mut self, key: FutexKey) {
        match key {
            Either::Left(priv_key) => {
                let prev = self.private_futexes.insert(
                    priv_key,
                    Futex {
                        waiters: VecDeque::new(),
                    },
                );
                debug_assert!(
                    prev.is_none(),
                    "futex already exists for private key {:?}, this should never happen",
                    priv_key
                );
            },
            Either::Right(shared_key) => {
                let prev = self.shared_futexes.insert(
                    shared_key,
                    Futex {
                        waiters: VecDeque::new(),
                    },
                );
                debug_assert!(
                    prev.is_none(),
                    "futex already exists for shared key {:?}, this should never happen",
                    shared_key
                );
            },
        }
    }

    fn exist_futex(&self, key: FutexKey) -> bool {
        match key {
            Either::Left(priv_key) => self.private_futexes.contains_key(&priv_key),
            Either::Right(shared_key) => self.shared_futexes.contains_key(&shared_key),
        }
    }

    fn get_futex(&mut self, key: FutexKey) -> Option<&mut Futex> {
        match key {
            Either::Left(priv_key) => self.private_futexes.get_mut(&priv_key),
            Either::Right(shared_key) => self.shared_futexes.get_mut(&shared_key),
        }
    }

    fn get_2_futex(
        &mut self,
        key1: FutexKey,
        key2: FutexKey,
    ) -> (Option<&mut Futex>, Option<&mut Futex>) {
        match (key1, key2) {
            (Either::Left(priv_key1), Either::Left(priv_key2)) => {
                let [futex1, futex2] = self
                    .private_futexes
                    .get_disjoint_mut([&priv_key1, &priv_key2]);
                (futex1, futex2)
            },
            (Either::Right(shared_key1), Either::Right(shared_key2)) => {
                let [futex1, futex2] = self
                    .shared_futexes
                    .get_disjoint_mut([&shared_key1, &shared_key2]);
                (futex1, futex2)
            },
            (Either::Left(priv_key1), Either::Right(shared_key2)) => {
                let futex1 = self.private_futexes.get_mut(&priv_key1);
                let futex2 = self.shared_futexes.get_mut(&shared_key2);
                (futex1, futex2)
            },
            (Either::Right(shared_key1), Either::Left(priv_key2)) => {
                let futex1 = self.shared_futexes.get_mut(&shared_key1);
                let futex2 = self.private_futexes.get_mut(&priv_key2);
                (futex1, futex2)
            },
        }
    }

    fn remove_futex(&mut self, key: FutexKey) {
        match key {
            Either::Left(priv_key) => {
                let removed = self.private_futexes.remove(&priv_key);
                debug_assert!(
                    removed.is_some(),
                    "futex not found for private key {:?}, this should never happen",
                    priv_key
                );
            },
            Either::Right(shared_key) => {
                let removed = self.shared_futexes.remove(&shared_key);
                debug_assert!(
                    removed.is_some(),
                    "futex not found for shared key {:?}, this should never happen",
                    shared_key
                );
            },
        }
    }
}

/// **LOCK ORDERING**
/// [UserSpace] -> [FUTEX_SET]
///
/// [Mutex] instead of spin-based lock. TODO: explain why.
static FUTEX_SET: Lazy<Mutex<FutexSet>> = Lazy::new(|| {
    Mutex::new(FutexSet {
        private_registry: HashMap::new(),
        private_futexes: HashMap::new(),
        shared_registry: HashMap::new(),
        shared_futexes: HashMap::new(),
    })
});

/// Given a user space address, calculate it to a futex key.
///
/// We intentionally use 'calc' instead of 'get' or 'find' to indicate that the
/// validness of the returned key (i.e. whether there do exist a futex
/// associated with the key) is not guaranteed.
///
/// ## Locks
/// - [FUTEX_SET]
/// - [UserSpaceHandle]
fn calc_futex_key(usp_handle: &Arc<UserSpaceHandle>, addr: VirtAddr) -> Result<FutexKey, SysError> {
    debug_assert!(
        addr.get().is_multiple_of(4),
        "futex address must be 4-byte aligned, this should already be validated by caller"
    );

    let (usp_addr, vmo, vmo_offset, fork_policy) = {
        let usp = &*usp_handle.lock();
        let vma = usp.find_vma(addr).ok_or(SysError::NotMapped)?;

        let vma_pidx = vma.vmo_pidx(vma.range().start()) as u64;
        let vmo_offset = (addr.get() - vma.range().start().get()
            + vma_pidx * PagingArch::PAGE_SIZE_BYTES as u64) as usize;

        (
            usp as *const _ as u64,
            vma.backing().clone(),
            vmo_offset,
            vma.on_fork(),
        )
    };

    let mut set = FUTEX_SET.lock();
    match fork_policy {
        ForkPolicy::CopyOnWrite => {
            let uspace_id;

            match set.private_registry.entry(usp_addr) {
                Entry::Occupied(mut exist) => {
                    let (stable_id, weak) = exist.get();
                    if let Some(strong) = weak.upgrade() {
                        debug_assert!(
                            Arc::ptr_eq(&strong, usp_handle),
                            "different user space handle with the same address, this should never happen"
                        );
                        // stable id already exists, reuse it.
                        uspace_id = *stable_id;
                    } else {
                        // stale entry, replace it.
                        uspace_id = alloc_uspace_id();
                        exist.insert((uspace_id, Arc::downgrade(usp_handle)));
                    }
                },
                Entry::Vacant(mut vacant) => {
                    // no entry, insert a new one.
                    uspace_id = alloc_uspace_id();
                    vacant.insert((uspace_id, Arc::downgrade(usp_handle)));
                },
            }

            Ok(Either::Left(PrivateFutexKey {
                uspace_id,
                word_idx: (addr.get() as usize) >> 2,
            }))
        },
        ForkPolicy::Shared => {
            let so_id;
            // note: we take the data pointer of vmo as its identity, which is guaranteed to
            // be unique for different vmos.
            let vmo_addr = Arc::as_ptr(&vmo) as *const () as u64;

            // almost the same as above.
            match set.shared_registry.entry(vmo_addr) {
                Entry::Occupied(mut exist) => {
                    let (stable_id, weak) = exist.get();
                    if let Some(strong) = weak.upgrade() {
                        debug_assert!(
                            Arc::ptr_eq(&strong, &vmo),
                            "different vmo with the same address, this should never happen"
                        );
                        // stable id already exists, reuse it.
                        so_id = *stable_id;
                    } else {
                        // stale entry, replace it.
                        so_id = alloc_shared_object_id();
                        exist.insert((so_id, Arc::downgrade(&vmo)));
                    }
                },
                Entry::Vacant(mut vacant) => {
                    // no entry, insert a new one.
                    so_id = alloc_shared_object_id();
                    vacant.insert((so_id, Arc::downgrade(&vmo)));
                },
            }

            Ok(Either::Right(SharedFutexKey {
                so_id,
                word_idx: vmo_offset >> 2,
            }))
        },
    }
}

/// **[FUTEX_SET] is locked when executing the closure!**
fn with_futex<F, R>(key: FutexKey, create: bool, f: F) -> Option<R>
where
    F: FnOnce(&mut Futex) -> R,
{
    let mut set = FUTEX_SET.lock();
    if create {
        if !set.exist_futex(key) {
            set.create_futex(key);
        }
    }
    let futex = set.get_futex(key)?;
    let ret = f(futex);
    if futex.waiters.is_empty() {
        // no need to keep the futex in the map.
        set.remove_futex(key);
    }
    Some(ret)
}

/// **[FUTEX_SET] is locked when executing the closure!**
///
/// This might seems ugly and redundant. but XXX.
fn with_2_futex<F, R>(key1: FutexKey, key2: FutexKey, create2: bool, f: F) -> Option<R>
where
    F: FnOnce(&mut Futex, &mut Futex) -> R,
{
    let mut set = FUTEX_SET.lock();

    if create2 {
        if !set.exist_futex(key2) {
            set.create_futex(key2);
        }
    }

    let (futex1, futex2) = set.get_2_futex(key1, key2);
    let (Some(futex1), Some(futex2)) = (futex1, futex2) else {
        // at least one of the futex doesn't exist, just return None.
        return None;
    };

    let ret = f(futex1, futex2);

    let (empty1, empty2) = (futex1.waiters.is_empty(), futex2.waiters.is_empty());

    if empty1 {
        // no need to keep the futex in the map.
        set.remove_futex(key1);
    }
    if empty2 {
        // no need to keep the futex in the map.
        set.remove_futex(key2);
    }
    Some(ret)
}

/// Wake up at most `n_waiters` waiters waiting on the futex associated with the
/// given user space address.
///
/// This function ignores bitset.
pub fn wake_at(
    usp_handle: &Arc<UserSpaceHandle>,
    addr: VirtAddr,
    n_waiters: usize,
) -> Result<usize, SysError> {
    if addr.get() % 4 != 0 {
        kdebugln!(
            "futex: invalid futex address {:#x?}, must be 4-byte aligned",
            addr
        );
        return Err(SysError::InvalidArgument);
    }

    let key = calc_futex_key(usp_handle, addr)?;

    match with_futex(key, false, |futex| {
        let mut n_woken = 0;
        while n_woken < n_waiters {
            if let Some(waiter) = futex.waiters.pop_front() {
                waiter.woken.store(true, Ordering::Release);
                waiter.futex_available.publish(1, true);
                n_woken += 1;
            } else {
                break;
            }
        }
        n_woken
    }) {
        Some(n) => Ok(n),
        None => Ok(0), // no futex, no waiters.
    }
}

/// Called when a task exits, to clean up its futexes.
pub fn exit_robust_list() -> Result<(), SysError> {
    fn handle_futex_death(word_addr: VirtAddr) -> Result<(), SysError> {
        if word_addr.get() % 4 != 0 {
            knoticeln!(
                "futex: invalid futex address {:#x?} in robust list, must be 4-byte aligned, skip cleaning up this futex",
                word_addr
            );
            return Ok(());
        }

        let task = get_current_task();
        kdebugln!(
            "handling futex death for {}, futex address {:#x?}",
            task.tid(),
            word_addr
        );
        let usp_handle = task.clone_uspace_handle();
        let key = calc_futex_key(&usp_handle, word_addr)?;

        let mut usp = usp_handle.lock();

        // note we use `true` here.
        let Some(ret) = with_futex(key, true, |futex| {
            usp.inject_page_fault(word_addr, PageFaultType::Write)?;
            let atomic_view =
                unsafe { (word_addr.as_ptr_mut() as *mut AtomicU32).as_ref().unwrap() };

            let final_val = loop {
                let old_val = atomic_view.load(Ordering::Acquire);
                let futex_tid = old_val & FUTEX_TID_MASK;
                if futex_tid != task.tid().get() {
                    knoticeln!(
                        "futex: futex tid {} does not match current task tid {}, skip waking up waiters",
                        futex_tid,
                        task.tid().get()
                    );
                    return Ok(());
                }
                if atomic_view
                    .compare_exchange(
                        old_val,
                        old_val | FUTEX_OWNER_DIED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    break old_val | FUTEX_OWNER_DIED;
                }
            };

            if final_val & FUTEX_WAITERS != 0 {
                // wake up a waiter.
                if let Some(waiters) = futex.waiters.pop_front() {
                    waiters.woken.store(true, Ordering::Release);
                    waiters.futex_available.publish(1, true);
                }
            }

            Ok::<(), SysError>(())
        }) else {
            kdebugln!(
                "futex: no futex found for key {:?}, skip waking up waiters",
                key
            );
            return Ok(());
        };

        Ok(())
    }

    /// TODO: make this a kconfig item.
    const ROBUST_LIST_LENGTH_LIMIT: usize = 2048;

    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();
    let Some(head_ptr) = task.robust_list() else {
        // no robust futex, nothing to clean up.
        return Ok(());
    };

    let RobustListHead {
        list,
        futex_offset,
        list_op_pending,
    } = {
        let mut usp = usp_handle.lock();
        let Ok(head_ptr) = UserReadPtr::<RobustListHead>::try_new(head_ptr, &mut usp) else {
            knoticeln!(
                "futex: invalid robust list head pointer {:#x?}, skip cleaning up futexes",
                head_ptr
            );
            return Ok(());
        };
        head_ptr.read()
    };

    if !list_op_pending.is_null() {
        handle_futex_death(user_addr((list_op_pending as i64 + futex_offset) as u64).map_err(|e| {
            knoticeln!(
                "futex: invalid futex address in robust list {:#x?}: {:?}, skip cleaning up this futex",
                list_op_pending,
                e
            );
            e
        })?)?;
    }

    let mut count = 0;
    let mut curr_ptr = list.next;
    loop {
        if count >= ROBUST_LIST_LENGTH_LIMIT {
            knoticeln!(
                "futex: robust list length exceeds the limit {}, stop cleaning up futexes",
                ROBUST_LIST_LENGTH_LIMIT
            );
            break;
        }

        if curr_ptr.is_null() || curr_ptr as u64 == head_ptr.get() {
            // end of the list.
            break;
        }

        let curr_word_addr = user_addr((curr_ptr as i64 + futex_offset) as u64).map_err(|e| {
            knoticeln!(
                "futex: invalid futex address in robust list {:#x?}: {:?}, skip cleaning up this futex",
                curr_ptr,
                e
            );
            e
        })?;
        handle_futex_death(curr_word_addr)?;
        count += 1;

        // current done.
        {
            let mut usp = usp_handle.lock();
            match UserReadPtr::<RobustList>::try_new(VirtAddr::new(curr_ptr as u64), &mut usp) {
                Ok(ptr) => curr_ptr = ptr.read().next,
                Err(e) => {
                    knoticeln!(
                        "futex: invalid robust list entry pointer {:#x?}, stop cleaning up futexes: {:?}",
                        curr_ptr,
                        e
                    );
                    break;
                },
            }
        }
    }

    Ok(())
}
