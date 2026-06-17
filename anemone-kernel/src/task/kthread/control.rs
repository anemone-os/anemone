use crate::prelude::*;

#[derive(Debug)]
pub(super) struct KThreadControl {
    phase: SpinLock<KThreadPhase>,
    wake: Event,
    exited: Event,
    /// Persistent predicate/result paired with `exited`.
    ///
    /// `Event` is a wake edge, not storage. Keeping the public completion
    /// result here lets `has_exited()` and `wait_exited()` observe external
    /// completion instead of treating internal `phase == Exited(_)` as the
    /// handle-visible lifecycle boundary.
    external_result: SpinLock<Option<i32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KThreadPhase {
    Running,
    StopRequested,
    Exited(i32),
}

impl KThreadControl {
    pub(super) fn new() -> Self {
        Self {
            phase: SpinLock::new(KThreadPhase::Running),
            wake: Event::new(),
            exited: Event::new(),
            external_result: SpinLock::new(None),
        }
    }

    pub(super) fn request_stop(&self) {
        let mut phase = self.phase.lock();
        match *phase {
            KThreadPhase::Running => *phase = KThreadPhase::StopRequested,
            KThreadPhase::StopRequested | KThreadPhase::Exited(_) => {},
        }
    }

    pub(super) fn wake(&self) {
        self.wake.publish(usize::MAX, true);
    }

    pub(super) fn should_stop(&self) -> bool {
        matches!(
            *self.phase.lock(),
            KThreadPhase::StopRequested | KThreadPhase::Exited(_)
        )
    }

    pub(super) fn wait_until<P>(&self, predicate: P)
    where
        P: Fn() -> bool,
    {
        self.wake
            .listen_uninterruptible(false, || self.should_stop() || predicate());
    }

    pub(in crate::task) fn complete_returned_entry(&self, code: i32) {
        {
            let mut phase = self.phase.lock();
            match *phase {
                KThreadPhase::Running | KThreadPhase::StopRequested => {
                    *phase = KThreadPhase::Exited(code);
                },
                KThreadPhase::Exited(_) => {
                    panic!("kthread exit result completed more than once");
                },
            }
        }
        self.wake();
    }

    pub(super) fn wait_exited(&self) -> i32 {
        self.exited
            .listen_uninterruptible(false, || self.external_result.lock().is_some());
        self.external_result
            .lock()
            .as_ref()
            .copied()
            .expect("kthread exited event observed without exit result")
    }

    pub(super) fn has_exited(&self) -> bool {
        self.external_result.lock().is_some()
    }

    pub(in crate::task) fn publish_external_exit(&self, code: i32) {
        {
            let mut external_result = self.external_result.lock();
            assert!(
                external_result.is_none(),
                "kthread external completion published more than once"
            );
            *external_result = Some(code);
        }
        self.exited.publish(usize::MAX, true);
    }
}
