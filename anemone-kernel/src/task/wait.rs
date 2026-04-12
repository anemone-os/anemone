use core::fmt::Debug;

use alloc::{boxed::Box, collections::vec_deque::VecDeque, sync::Arc};

use crate::prelude::*;

pub struct WaitInfo<T> {
    waiter: Arc<Task>,
    handler: Box<dyn FnOnce(&T) + 'static + Send + Sync>,
}

impl<T> Debug for WaitInfo<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WaitInfo")
            .field("waiter", &self.waiter)
            .finish()
    }
}

#[derive(Debug)]
pub struct WaitQueue<T> {
    queue: SpinLock<VecDeque<WaitInfo<T>>>,
}

impl<T> WaitQueue<T> {
    pub fn new() -> Self {
        WaitQueue {
            queue: SpinLock::new(VecDeque::new()),
        }
    }

    pub fn wake(&self, data: &T, exclusive: bool) {
        if exclusive {
            if let Some(waiter) = self.queue.lock().pop_front() {
                (waiter.handler)(data);
                //kdebugln!("wake up {}", waiter.waiter.tid());
                add_to_ready(waiter.waiter);
            }
        } else {
            while let Some(waiter) = self.queue.lock().pop_front() {
                (waiter.handler)(data);
                //kdebugln!("wake up {}", waiter.waiter.tid());
                add_to_ready(waiter.waiter);
            }
        }
    }
}

impl<T: Clone + Sync + Send + 'static> WaitQueue<T> {
    /// **This function will get the lock before check the condition. Need to
    /// prevent deadlocks.**
    pub fn wait_if<E>(
        &self,
        interruptible: bool,
        condition: impl FnOnce() -> Result<(), E>,
    ) -> Result<T, E> {
        let task = clone_current_task();
        let mut locked = self.queue.lock();
        match condition() {
            Ok(()) => {
                //kdebugln!("{} sleep", task.tid());
                let res: &mut Option<T> = Box::leak(Box::new(None));
                let ptr = res as *mut Option<T>;
                
                // set the task status before drop the lock, otherwise race conditions may
                // happen.
                task.set_status(TaskStatus::Waiting { interruptible });
                locked.push_back(WaitInfo {
                    waiter: task,
                    handler: Box::new(move |data| {
                        *res = Some(data.clone());
                    }),
                });
                drop(locked);
                sleep_as_waiting(interruptible);
                //kdebugln!("{} awaken", current_task_id());
                let boxed = unsafe { Box::from_raw(ptr) };
                let res = (*boxed).unwrap();
                Ok(res)
            },
            Err(e) => Err(e),
        }
    }
}
