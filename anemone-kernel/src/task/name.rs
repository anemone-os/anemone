use anemone_abi::capability::linux::TASK_COMM_LEN;

use crate::prelude::*;

/// The single authoritative name representation owned by a task.
///
/// Kernel labels remain trusted UTF-8 diagnostics. User names are raw bytes
/// because `PR_SET_NAME` accepts non-UTF-8 input; their length excludes the
/// trailing NUL required only at the Linux ABI boundary. The variant selects
/// the name representation; [`TaskFlags`] remains authoritative for task kind.
#[derive(Clone)]
pub(super) enum TaskName {
    KernelLabel(Box<str>),
    UserComm(Box<[u8]>),
}

impl TaskName {
    pub(super) fn kernel(label: &str) -> Self {
        Self::KernelLabel((String::from("@kernel/") + label).into_boxed_str())
    }

    pub(super) fn idle() -> Self {
        Self::KernelLabel(Box::from("@idle"))
    }

    pub(super) fn user_from_exec(executable_name: &str) -> Self {
        let bytes = executable_name.as_bytes();
        let len = bytes.len().min(TASK_COMM_LEN - 1);
        Self::UserComm(Box::from(&bytes[..len]))
    }

    fn diagnostic_name(&self) -> Box<str> {
        match self {
            Self::KernelLabel(label) => label.clone(),
            Self::UserComm(comm) => {
                let mut name = String::from("@user/");
                name.push_str(&String::from_utf8_lossy(comm));
                name.into_boxed_str()
            },
        }
    }

    fn comm(&self) -> Box<[u8]> {
        match self {
            Self::KernelLabel(label) => {
                let label = label.strip_prefix("@kernel/").unwrap_or(label);
                let basename = label.rsplit('/').next().unwrap_or(label);
                let bytes = basename.as_bytes();
                Box::from(&bytes[..bytes.len().min(TASK_COMM_LEN - 1)])
            },
            Self::UserComm(comm) => comm.clone(),
        }
    }
}

impl Task {
    /// Get the task's diagnostic name. This introduces a heap allocation.
    pub fn name(&self) -> Box<str> {
        self.name.read().diagnostic_name()
    }

    /// Snapshot the Linux-visible task `comm` without its trailing NUL.
    pub(crate) fn comm(&self) -> Box<[u8]> {
        self.name.read().comm()
    }

    /// Replace the current user task's Linux-visible `comm`.
    pub(in crate::task) fn set_comm(&self, comm: Box<[u8]>) {
        assert!(
            comm.len() < TASK_COMM_LEN && !comm.contains(&0),
            "task comm must contain at most 15 non-NUL bytes"
        );

        // Keep the task-kind authority and representation invariant under the
        // documented flags -> name lock order.
        let flags = self.flags.read();
        let mut name = self.name.write();
        assert!(
            !flags.is_kernel(),
            "a pure kernel task cannot accept a user task comm"
        );
        let TaskName::UserComm(current) = &mut *name else {
            panic!("a user task must own a user task comm");
        };
        *current = comm;
    }

    pub(in crate::task) fn name_snapshot(&self) -> TaskName {
        self.name.read().clone()
    }
}
