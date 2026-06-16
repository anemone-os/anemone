use crate::{prelude::*, task::exit::kernel_exit, utils::any_opaque::AnyOpaque};

use super::{KThreadCtx, control::KThreadControl};

pub type KThreadEntry = fn(KThreadCtx, AnyOpaque) -> i32;

pub(in crate::task) struct KThreadTaskLocal {
    pub(super) control: Arc<KThreadControl>,
    pub(super) launch: SpinLock<Option<KThreadLaunch>>,
}

impl KThreadTaskLocal {
    pub(super) fn new(control: Arc<KThreadControl>, launch: Option<KThreadLaunch>) -> Self {
        Self {
            control,
            launch: SpinLock::new(launch),
        }
    }
}

pub(super) struct KThreadLaunch {
    pub(super) entry: KThreadEntry,
    pub(super) arg: AnyOpaque,
}

impl KThreadLaunch {
    pub(super) fn new(entry: KThreadEntry, arg: AnyOpaque) -> Self {
        Self { entry, arg }
    }
}

pub(super) fn kthread_entry_shim() -> ! {
    let task = get_current_task();
    let (control, launch) = task.take_kthread_launch();
    let ctx = KThreadCtx::new(control.clone());

    let code = if ctx.should_stop() {
        -EINTR
    } else {
        (launch.entry)(ctx, launch.arg)
    };
    control.complete_returned_entry(code);

    // Stage 4 owns the dedicated kthread exit path and `kernel_exit()` guard
    // changes. Until that gate, the shim still uses the full user-process exit
    // tail after completing the kthread result expected by the current guard.
    kernel_exit(ExitCode::Exited(code as i8))
}
