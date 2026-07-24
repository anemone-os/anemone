use crate::{prelude::*, task::sig::SigNo};

/// Signal-owned classification used by TTY background-operation policy.
///
/// This deliberately exports neither the task mask nor the shared disposition;
/// both remain Signal truth and are sampled only for this one decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TtyJobControlDisposition {
    BlockedOrIgnored,
    Actionable,
}

impl Task {
    pub(crate) fn tty_job_control_disposition(&self, no: SigNo) -> TtyJobControlDisposition {
        assert!(
            matches!(no, SigNo::SIGTTIN | SigNo::SIGTTOU),
            "TTY disposition query used for a non-job-control signal"
        );
        if self.is_current_sig_mask_blocking(no)
            || self
                .sig_disposition
                .read()
                .get_disposition(no)
                .action
                .is_ignored()
        {
            TtyJobControlDisposition::BlockedOrIgnored
        } else {
            TtyJobControlDisposition::Actionable
        }
    }
}
