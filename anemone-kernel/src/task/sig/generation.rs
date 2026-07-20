use crate::{
    prelude::*,
    task::{
        Task, ThreadGroup,
        sig::{SigNo, Signal, disposition::SignalDisposition, set::SigSet},
    },
};

impl Task {
    /// Send a signal to this task (I.e. let this task receive a signal).
    /// - For unreliable signals, we just set the bit in `sig_pending`. if the
    ///   slot is already occupied, the old signal will be overwritten, and lost
    ///   forever.
    /// - For realtime signals, new signals will be pushed to the back of the
    ///   queue, and they won't be lost.
    ///
    /// If the signal is masked, task won't be notified, except for [SIGKILL]
    /// and [SIGSTOP].
    ///
    /// If the disposition of the signal satisfies [SignalAction::is_ignored],
    /// the signal won't be delivered, even if it is unmasked.
    pub fn recv_signal(self: &Arc<Self>, signal: Signal) {
        kdebugln!("task {} recv_signal: {:?}", self.tid(), signal);
        let no = signal.no;

        let disp = self.sig_disposition.read().get_disposition(no);
        if disp.action.is_ignored() {
            kdebugln!("signal {:?} is ignored by the task, not queuing", no);
            return;
        }

        self.sig_pending.lock().push_signal(signal);

        if self.is_current_sig_mask_blocking(no) && !matches!(no, SigNo::SIGKILL | SigNo::SIGSTOP) {
            // signal masked. nothing to do now, just wait for the task to
            // unmask it.
        } else {
            kdebugln!("signal {:?} is not masked, notifying task", no);
            notify(
                self,
                if matches!(no, SigNo::SIGKILL | SigNo::SIGSTOP) {
                    true
                } else {
                    false
                },
            );
        }
    }
}

impl ThreadGroup {
    /// Get the signal disposition of this thread group.
    ///
    /// Internally, this relies on the fact that
    /// [super::super::clone::CloneFlags::THREAD] must be used with
    /// [clone::CloneFlags::SIGHAND], so that all threads in the same thread
    /// group share the same [SignalDisposition].
    ///
    /// Return `None` if this thread group has no members, (i.e. the thread
    /// group is waiting to be reaped).
    pub fn signal_disposition(&self) -> Option<Arc<NoIrqRwLock<SignalDisposition>>> {
        self.get_members()
            .into_iter()
            .next()
            .map(|member| member.sig_disposition.clone())
    }

    /// Send a signal to this thread group.
    ///
    /// If the disposition of the signal satisfies [SignalAction::is_ignored],
    /// the signal won't be delivered.
    pub fn recv_signal(&self, signal: Signal) {
        let no = signal.no;
        let members = self.get_members();

        let Some(first_member) = members.first() else {
            knoticeln!(
                "trying to send signal {:?} to a thread group with no members",
                no
            );
            return;
        };
        let disp = first_member.sig_disposition.read().get_disposition(no);

        if disp.action.is_ignored() {
            kdebugln!(
                "signal {:?} is ignored by the thread group, not queuing",
                no
            );
            return;
        }

        {
            let inner = self.inner.read();
            inner.sig_pending.lock().push_signal(signal);
        }

        for member in members {
            if member.is_current_sig_mask_blocking(no)
                && !matches!(no, SigNo::SIGKILL | SigNo::SIGSTOP)
            {
                // signal masked. nothing to do now, just wait for the task to
                // unmask it.
            } else {
                notify(
                    &member,
                    if matches!(no, SigNo::SIGKILL | SigNo::SIGSTOP) {
                        true
                    } else {
                        false
                    },
                );
            }
        }
    }

    /// Flush pending signals in the given set.
    ///
    /// Both each member's private pending signals and shared pending signals
    /// will be flushed.
    ///
    /// TODO: [SignalDisposition] actually can be shared by multiple thread
    /// groups. currently we restrict clone's flags to avoid this. But later we
    /// should support that, and this method won't be put in [ThreadGroup].
    pub fn flush_specific_signals(&self, set: SigSet) {
        {
            let inner = self.inner.write();
            let mut pending = inner.sig_pending.lock();
            pending.flush_specific(set);
        }
        for member in self.get_members() {
            member.sig_pending.lock().flush_specific(set);
        }
    }
}
