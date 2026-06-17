//! Lightweight kernel thread creation and lifecycle core.
//!
//! `kthreadd` owns only the create transaction. Ordinary kthread lifecycle is
//! owned by `KThreadControl` and exposed to subsystem owners through a strong
//! `KThreadHandle`; scheduler state remains owned by `TaskSchedState`.

use crate::prelude::*;

mod control;
mod ctx;
mod entry;
mod handle;
mod kthreadd;
mod spawn;

pub use ctx::KThreadCtx;
pub use entry::KThreadEntry;
pub use handle::KThreadHandle;
pub use kthreadd::init_kthreadd;
pub use spawn::{KThreadBuilder, KThreadPlacement};

pub(in crate::task) use entry::KThreadTaskLocal;

use control::KThreadControl;
use entry::KThreadLaunch;

impl Task {
    pub(super) fn install_kthread(&self, local: KThreadTaskLocal) {
        let mut slot = self.kthread.lock();
        assert!(
            slot.is_none(),
            "kthread installed more than once for task {}",
            self.tid()
        );
        *slot = Some(local);
    }

    pub fn kthread(&self) -> Option<KThreadHandle> {
        self.kthread
            .lock()
            .as_ref()
            .map(|local| KThreadHandle::new(local.control.clone()))
    }

    pub(in crate::task) fn complete_kthread_returned_entry(&self, code: i32) {
        let slot = self.kthread.lock();
        let local = slot
            .as_ref()
            .expect("kthread exit requires task-local kthread state");
        local.control.complete_returned_entry(code);
    }

    pub(in crate::task) fn publish_kthread_external_exit(&self, code: i32) {
        let slot = self.kthread.lock();
        let local = slot
            .as_ref()
            .expect("kthread external exit requires task-local kthread state");
        local.control.publish_external_exit(code);
    }

    fn has_kthread_attachment(&self) -> bool {
        self.kthread.lock().is_some()
    }

    fn take_kthread_launch(&self) -> (Arc<KThreadControl>, KThreadLaunch) {
        let slot = self.kthread.lock();
        let local = slot.as_ref().unwrap_or_else(|| {
            panic!(
                "ordinary kthread {} is missing task-local state",
                self.tid()
            )
        });
        let launch = local.launch.lock().take().unwrap_or_else(|| {
            panic!(
                "ordinary kthread {} launch slot was already taken",
                self.tid()
            )
        });
        (local.control.clone(), launch)
    }
}
