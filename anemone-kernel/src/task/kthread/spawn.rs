use crate::{prelude::*, utils::any_opaque::AnyOpaque};

use super::{
    KThreadEntry, KThreadHandle,
    entry::KThreadLaunch,
    kthreadd::{self, SpawnReply, SpawnRequest},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KThreadPlacement {
    Any,
    OnCpu(CpuId),
}

/// Builder collecting creation policy for one kthread.
#[derive(Debug)]
pub struct KThreadBuilder {
    name: Box<str>,
    placement: KThreadPlacement,
}

impl KThreadBuilder {
    /// Create a builder with a stable debug name.
    pub fn new(name: impl Into<Box<str>>) -> Self {
        Self {
            name: name.into(),
            placement: KThreadPlacement::Any,
        }
    }

    pub fn placement(mut self, placement: KThreadPlacement) -> Self {
        if let KThreadPlacement::OnCpu(cpu) = placement {
            let ncpus = ncpus();
            assert!(cpu.logical_id() < ncpus, "kthread spawn: invalid {}", cpu);
        }
        self.placement = placement;
        self
    }

    /// Pin the new kthread to an initial CPU.
    pub fn cpu(self, cpu: CpuId) -> Self {
        self.placement(KThreadPlacement::OnCpu(cpu))
    }

    /// Submit creation to `kthreadd` and wait until the task is published and
    /// enqueued. `arg` is consumed by the transaction; failed spawns drop it.
    pub fn spawn(self, entry: KThreadEntry, arg: AnyOpaque) -> Result<KThreadHandle, SysError> {
        let reply = Arc::new(SpawnReply::new());
        let request = SpawnRequest {
            name: self.name,
            placement: self.placement,
            launch: KThreadLaunch::new(entry, arg),
            reply: reply.clone(),
        };
        kthreadd::submit(request, reply)
    }
}
