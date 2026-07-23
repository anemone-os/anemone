use crate::{prelude::*, task::sig::SigNo};

/// Signal-owned classification used by TTY background-operation policy.
///
/// This deliberately exports neither the task mask nor the shared disposition;
/// both remain Signal truth and are sampled only for this one decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TtySigttouDisposition {
    BlockedOrIgnored,
    Actionable,
}

impl Task {
    pub(crate) fn tty_sigttou_disposition(&self) -> TtySigttouDisposition {
        let no = SigNo::SIGTTOU;
        if self.is_current_sig_mask_blocking(no)
            || self
                .sig_disposition
                .read()
                .get_disposition(no)
                .action
                .is_ignored()
        {
            TtySigttouDisposition::BlockedOrIgnored
        } else {
            TtySigttouDisposition::Actionable
        }
    }
}
