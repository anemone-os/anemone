//! Task cpu usage tracking.

use crate::prelude::*;

/// Privilege Level of a control flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Privilege {
    Kernel = 0,
    User = 1,
}

#[derive(Debug, Clone, Copy)]
struct RunningFlow {
    prv: Privilege,
    running_since: u64,
}

/// use raw monotonic time to avoid precision loss.
#[derive(Debug, Clone, Copy)]
pub struct TaskCpuUsage {
    user: u64,
    kernel: u64,
    running_flow: Option<RunningFlow>,
}

/// Thread-group level cpu usage.
#[derive(Debug, Clone, Copy)]
pub struct ThreadGroupCpuUsage {
    self_user: u64,
    self_kernel: u64,
    reaped_user: u64,
    reaped_kernel: u64,
}

macro_rules! gen_cpu_usage_getter {
    ($($name:ident,)*) => {
        $(
            paste::paste! {
                pub fn $name(&self) -> Duration {
                    duration_from_mono(self.$name)
                }
                pub fn [<$name _mono>](&self) -> u64 {
                    self.$name
                }
            }
        )*
    };
}

impl TaskCpuUsage {
    pub const ZERO: Self = Self {
        user: 0,
        kernel: 0,
        running_flow: None,
    };

    gen_cpu_usage_getter!(user, kernel,);
}

impl TaskCpuUsage {
    /// Settle current running flow, i.e. add the elapsed time since last switch
    /// to the corresponding self cpu usage.
    ///
    /// Returns current monotonic time.
    ///
    /// Panics if there is no running flow, which indicates a bug in caller
    /// code.
    #[track_caller]
    fn settle(&mut self) -> u64 {
        let now = monotonic_uptime();

        let Some(ref mut running_flow) = self.running_flow else {
            panic!("settle called while not running");
        };

        let delta = now - running_flow.running_since;
        match running_flow.prv {
            Privilege::User => self.user += delta,
            Privilege::Kernel => self.kernel += delta,
        }
        running_flow.running_since = now;

        now
    }

    fn on_switch_in(&mut self) {
        debug_assert!(
            self.running_flow.is_none(),
            "switching in while already running"
        );

        let now = monotonic_uptime();

        self.running_flow = Some(RunningFlow {
            // a task is always switched in with kernel privilege.
            prv: Privilege::Kernel,
            running_since: now,
        });
    }

    fn on_switch_out(&mut self) {
        self.settle();
        self.running_flow = None;
    }

    #[track_caller]
    fn on_prv_change(&mut self, to: Privilege) {
        self.settle();
        let Some(ref mut running_flow) = self.running_flow else {
            panic!("privilege change while not running");
        };
        match running_flow.prv {
            Privilege::User => debug_assert!(
                to == Privilege::Kernel,
                "invalid privilege change from user to user"
            ),
            Privilege::Kernel => debug_assert!(
                to == Privilege::User,
                "invalid privilege change from kernel to kernel"
            ),
        }
        running_flow.prv = to;
    }
}

impl Task {
    pub fn cpu_usage_snapshot(&self) -> TaskCpuUsage {
        let cpu_usage = self.cpu_usage.read_irqsave();
        let mut snapshot = *cpu_usage;
        if snapshot.running_flow.is_some() {
            snapshot.settle();
        }
        snapshot
    }

    pub fn on_switch_in(&self) {
        self.cpu_usage.write_irqsave().on_switch_in();
    }

    pub fn on_switch_out(&self) {
        self.cpu_usage.write_irqsave().on_switch_out();
    }

    #[track_caller]
    pub fn on_prv_change(&self, to: Privilege) {
        self.cpu_usage.write_irqsave().on_prv_change(to);
    }
}

impl ThreadGroupCpuUsage {
    pub const ZERO: Self = Self {
        self_user: 0,
        self_kernel: 0,
        reaped_user: 0,
        reaped_kernel: 0,
    };

    gen_cpu_usage_getter!(self_user, self_kernel, reaped_user, reaped_kernel,);

    /// Called when a child thread group is reaped, to add the cpu usage of the
    /// child to self reaped cpu usage.
    fn on_reap(&mut self, child: &ThreadGroupCpuUsage) {
        self.reaped_user += child.self_user + child.reaped_user;
        self.reaped_kernel += child.self_kernel + child.reaped_kernel;
    }

    fn accumulate_member_usage(&mut self, member: &TaskCpuUsage) {
        self.self_user += member.user;
        self.self_kernel += member.kernel;
    }
}

impl ThreadGroup {
    pub fn cpu_usage_snapshot(&self) -> ThreadGroupCpuUsage {
        let mut snapshot = self.inner.read_irqsave().cpu_usage;

        self.for_each_member(|member| {
            let member_cpu_usage = member.cpu_usage_snapshot();
            snapshot.accumulate_member_usage(&member_cpu_usage);
        });

        snapshot
    }

    pub fn on_reap(&self, child: &ThreadGroup) {
        let child_cpu_usage = child.cpu_usage_snapshot();
        // self.cpu_usage.write_irqsave().on_reap(&child_cpu_usage);
        self.inner
            .write_irqsave()
            .cpu_usage
            .on_reap(&child_cpu_usage);
    }

    /// Accumulate the cpu usage of a member task to thread group cpu usage.
    ///
    /// Called when a member task is detached from this thread group.
    ///
    /// Panics if the given task is not a member of this thread group.
    pub fn accumulate_member_usage(&self, member: &Task) {
        let member_cpu_usage = member.cpu_usage_snapshot();
        let mut inner = self.inner.write_irqsave();
        assert!(inner.members.contains(&member.tid()));
        inner.cpu_usage.accumulate_member_usage(&member_cpu_usage);
    }
}
