use crate::{
    prelude::*,
    task::{
        Task, ThreadGroup, ThreadGroupInner,
        jobctl::group::ContinueEpoch,
        sig::{
            PosixTimerSignalCompletion, PosixTimerSignalEnqueue, PosixTimerSignalRoute, SigNo,
            Signal, disposition::SignalDisposition, notify_signalfd_rechecks, set::SigSet,
        },
    },
};

pub(super) fn is_job_control_signal(no: SigNo) -> bool {
    matches!(
        no,
        SigNo::SIGSTOP | SigNo::SIGTSTP | SigNo::SIGTTIN | SigNo::SIGTTOU | SigNo::SIGCONT
    )
}

fn is_conditional_stop_signal(no: SigNo) -> bool {
    matches!(no, SigNo::SIGTSTP | SigNo::SIGTTIN | SigNo::SIGTTOU)
}

fn finish_timer_signal_flushes(signals: Vec<Signal>) {
    for mut signal in signals {
        signal.finish_timer_signal_handoff(PosixTimerSignalCompletion::Flushed);
    }
}

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

        if is_job_control_signal(no) {
            let Some(tg) = get_thread_group(&self.tgid()) else {
                // A sender can retain a Task Arc after topology detach. The
                // concrete task identity is no longer a signal target then.
                return;
            };
            tg.recv_job_control_signal(signal, JobControlSignalRoute::Private(self));
            return;
        }

        let disp = self.sig_disposition.read().get_disposition(no);
        if disp.action.is_ignored() {
            kdebugln!("signal {:?} is ignored by the task, not queuing", no);
            return;
        }

        self.sig_pending.lock().push_signal(signal);
        let recheck_routes =
            get_thread_group(&self.tgid()).map(|tg| tg.snapshot_signalfd_rechecks());
        // Publish private pending before making the conservative cache visible.
        self.rearm_signal_return_work();
        if let Some(routes) = recheck_routes {
            notify_signalfd_rechecks(routes);
        }

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

    /// Admit an exact-task timer control signal through the ThreadGroup's
    /// generation transaction while retaining private occurrence ownership.
    pub(super) fn enqueue_private_timer_job_control_signal(
        self: &Arc<Self>,
        thread_group: &Arc<ThreadGroup>,
        registration: &PosixTimerSignalRoute,
        no: SigNo,
        generation: u64,
        episode: u64,
        overrun: i32,
    ) -> PosixTimerSignalEnqueue {
        thread_group.enqueue_timer_job_control_signal_to(
            TimerJobControlSignalRoute::Private(self),
            registration,
            no,
            generation,
            episode,
            overrun,
        )
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

        if is_job_control_signal(no) {
            self.recv_job_control_signal(signal, JobControlSignalRoute::Shared);
            return;
        }

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
        let recheck_routes = self.snapshot_signalfd_rechecks();

        // Snapshot after publication. A member present now is rearmed below;
        // one joining later starts armed and cannot miss this shared pending.
        let members = self.get_members();
        for member in &members {
            member.rearm_signal_return_work();
        }
        notify_signalfd_rechecks(recheck_routes);

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
        let mut retired = {
            let inner = self.inner.write();
            let mut pending = inner.sig_pending.lock();
            pending.flush_specific(set)
        };
        let members = self.get_members();
        for member in &members {
            retired.extend(member.sig_pending.lock().flush_specific(set));
        }
        finish_timer_signal_flushes(retired);
    }

    /// Deliver a process-directed occurrence after revalidating the exact task
    /// identity originally resolved by the syscall. The ordinary occurrence
    /// remains shared; only its generation authority is task-specific.
    pub(in crate::task) fn recv_signal_for_exact_member(&self, signal: Signal, target: &Arc<Task>) {
        if is_job_control_signal(signal.no) {
            self.recv_job_control_signal(
                signal,
                JobControlSignalRoute::SharedForExactMember(target),
            );
        } else {
            self.recv_signal(signal);
        }
    }

    /// Deliver one process-group-selected occurrence. Control signals recheck
    /// the selector under the ThreadGroup owner before any cleanup or phase
    /// effect; ordinary signals retain the current snapshot semantics.
    pub(in crate::task) fn recv_signal_from_process_group(
        &self,
        signal: Signal,
        expected_pgid: Tid,
    ) {
        if is_job_control_signal(signal.no) {
            self.recv_job_control_signal(
                signal,
                JobControlSignalRoute::SharedForProcessGroup(expected_pgid),
            );
        } else {
            self.recv_signal(signal);
        }
    }

    /// Commit control-signal generation in the ThreadGroup ordering domain.
    /// A task-directed occurrence still has a group-wide control effect, while
    /// its ordinary mask/disposition occurrence remains task-private.
    fn recv_job_control_signal(&self, mut signal: Signal, route: JobControlSignalRoute<'_>) {
        let no = signal.no;
        assert!(is_job_control_signal(no));

        // Topology selects the exact live member objects before the child owner
        // admits any cleanup, phase mutation, or parent-visible report. The
        // helper releases every guard before the effects below.
        let Some(((notify_targets, retired), transition)) =
            self.with_child_status_transaction(|members, inner| {
                if !matches!(inner.status.life_cycle(), ThreadGroupLifeCycle::Alive)
                    || members.is_empty()
                {
                    // The last task is detached from topology before kernel_exit
                    // publishes terminal lifecycle. A sender may still hold this
                    // stale ThreadGroup Arc during that gap, but an empty group
                    // cannot admit cleanup, a control transition, or an ordinary
                    // SIGCONT occurrence.
                    return (
                        (Vec::new(), Vec::new()),
                        crate::task::jobctl::group::JobControlTransition::NONE,
                    );
                }

                if let Some(target) = route.exact_member() {
                    let exact_member = target.tgid() == self.tgid()
                        && members.iter().any(|member| Arc::ptr_eq(member, target));
                    if !exact_member {
                        // The sender resolved a task that no longer belongs to
                        // this exact ThreadGroup. No cleanup or phase effect is
                        // allowed after that relation becomes stale.
                        return (
                            (Vec::new(), Vec::new()),
                            crate::task::jobctl::group::JobControlTransition::NONE,
                        );
                    }
                }
                if let Some(expected_pgid) = route.expected_pgid()
                    && inner.pgid != Some(expected_pgid)
                {
                    // Process-group membership may change after the sender's
                    // snapshot. A departed ThreadGroup must not receive any
                    // control cleanup, occurrence, or phase side effect.
                    return (
                        (Vec::new(), Vec::new()),
                        crate::task::jobctl::group::JobControlTransition::NONE,
                    );
                }

                let opposite = if no != SigNo::SIGCONT {
                    SigSet::new_with_signos(&[SigNo::SIGCONT])
                } else {
                    SigSet::new_with_signos(&[
                        SigNo::SIGSTOP,
                        SigNo::SIGTSTP,
                        SigNo::SIGTTIN,
                        SigNo::SIGTTOU,
                    ])
                };
                let mut retired = inner.sig_pending.lock().flush_specific(opposite);
                for member in members {
                    // Reserved delivery is deliberately outside ordinary pending
                    // cleanup and remains owned by the target task.
                    retired.extend(member.sig_pending.lock().flush_specific(opposite));
                }

                let transition = match no {
                    SigNo::SIGSTOP => {
                        if self.tgid() == Tid::INIT {
                            // Global init admits the concrete generation and
                            // cleanup, but can never obtain stop authority.
                            crate::task::jobctl::group::JobControlTransition::NONE
                        } else {
                            let ThreadGroupInner {
                                members,
                                job_control,
                                ..
                            } = &mut *inner;
                            job_control
                                .as_mut()
                                .expect("jobctl: user ThreadGroup lacks control state")
                                .request_unconditional_stop(members, self.tgid(), no)
                        }
                    },
                    SigNo::SIGCONT => inner
                        .job_control
                        .as_mut()
                        .expect("jobctl: user ThreadGroup lacks control state")
                        .continue_generation(self.tgid()),
                    _ => {
                        assert!(is_conditional_stop_signal(no));
                        let epoch = inner
                            .job_control
                            .as_ref()
                            .expect("jobctl: user ThreadGroup lacks control state")
                            .continue_epoch();
                        signal.set_default_stop_epoch(epoch);
                        crate::task::jobctl::group::JobControlTransition::NONE
                    },
                };

                let notify_targets = if no == SigNo::SIGSTOP {
                    // SIGSTOP is consumed as control input and never enters an
                    // ordinary pending queue or force-completes an active wait.
                    Vec::new()
                } else {
                    let private_target = route.private_target();
                    let occurrence_owner = private_target
                        .cloned()
                        .or_else(|| members.first().cloned())
                        .expect("jobctl: live ThreadGroup has no signal disposition owner");
                    let disposition = occurrence_owner.sig_disposition.read().get_disposition(no);
                    // Linux preserves a blocked default SIGCONT occurrence so
                    // userspace can install a handler before unblocking it. A
                    // conditional stop with explicit SIG_IGN follows ordinary
                    // generation semantics and is discarded after cleanup.
                    let discard = if no == SigNo::SIGCONT {
                        disposition.action.is_explicit_ignore()
                            || disposition.action.is_default_ignore()
                                && !occurrence_owner.is_current_sig_mask_blocking(no)
                    } else {
                        assert!(is_conditional_stop_signal(no));
                        disposition.action.is_ignored()
                    };
                    if discard {
                        Vec::new()
                    } else if let Some(target) = private_target {
                        target.sig_pending.lock().push_signal(signal);
                        vec![target.clone()]
                    } else {
                        inner.sig_pending.lock().push_signal(signal);
                        members.to_vec()
                    }
                };
                ((notify_targets, retired), transition)
            })
        else {
            return;
        };
        let recheck_routes = if notify_targets.is_empty() {
            None
        } else {
            Some(self.snapshot_signalfd_rechecks())
        };
        // Pending and job-control phase publication precede rearm, which in
        // turn precedes the existing job-control wake and notification.
        for member in &notify_targets {
            member.rearm_signal_return_work();
        }
        self.finish_job_control_transition(transition);
        // Opposite-class cleanup may retire POSIX timer occurrences sharing a
        // control signal number. Complete their owner callbacks only after the
        // ThreadGroup generation transaction has released every guard.
        finish_timer_signal_flushes(retired);
        if let Some(routes) = recheck_routes {
            notify_signalfd_rechecks(routes);
        }

        for member in notify_targets {
            if !member.is_current_sig_mask_blocking(no) {
                notify(&member, false);
            }
        }
    }

    /// Admit a POSIX-timer control signal without moving its occurrence into
    /// ordinary standard-signal storage. Job-control effects and opposite-class
    /// cleanup remain one ThreadGroup transaction; any deliverable occurrence
    /// stays in the timer's preallocated signal slot.
    pub(super) fn enqueue_timer_job_control_signal(
        &self,
        registration: &PosixTimerSignalRoute,
        no: SigNo,
        generation: u64,
        episode: u64,
        overrun: i32,
    ) -> PosixTimerSignalEnqueue {
        self.enqueue_timer_job_control_signal_to(
            TimerJobControlSignalRoute::Shared,
            registration,
            no,
            generation,
            episode,
            overrun,
        )
    }

    fn enqueue_timer_job_control_signal_to(
        &self,
        route: TimerJobControlSignalRoute<'_>,
        registration: &PosixTimerSignalRoute,
        no: SigNo,
        generation: u64,
        episode: u64,
        overrun: i32,
    ) -> PosixTimerSignalEnqueue {
        let Some(((outcome, notify_targets, no, retired), transition)) = self
            .with_child_status_transaction(|members, inner| {
                if !matches!(inner.status.life_cycle(), ThreadGroupLifeCycle::Alive)
                    || members.is_empty()
                {
                    return (
                        (
                            PosixTimerSignalEnqueue::TargetExited,
                            Vec::new(),
                            no,
                            Vec::new(),
                        ),
                        crate::task::jobctl::group::JobControlTransition::NONE,
                    );
                }

                if let Some(target) = route.private_target()
                    && (target.tgid() != self.tgid()
                        || !members.iter().any(|member| Arc::ptr_eq(member, target)))
                {
                    return (
                        (
                            PosixTimerSignalEnqueue::TargetExited,
                            Vec::new(),
                            no,
                            Vec::new(),
                        ),
                        crate::task::jobctl::group::JobControlTransition::NONE,
                    );
                }

                let registration_no = match route {
                    TimerJobControlSignalRoute::Shared => {
                        let pending = inner.sig_pending.lock();
                        let Some(no) = registration.registered_no_locked(&pending) else {
                            return (
                                (
                                    PosixTimerSignalEnqueue::TargetExited,
                                    Vec::new(),
                                    no,
                                    Vec::new(),
                                ),
                                crate::task::jobctl::group::JobControlTransition::NONE,
                            );
                        };
                        no
                    },
                    TimerJobControlSignalRoute::Private(target) => {
                        let pending = target.sig_pending.lock();
                        let Some(no) = registration.registered_no_locked(&pending) else {
                            return (
                                (
                                    PosixTimerSignalEnqueue::TargetExited,
                                    Vec::new(),
                                    no,
                                    Vec::new(),
                                ),
                                crate::task::jobctl::group::JobControlTransition::NONE,
                            );
                        };
                        no
                    },
                };
                assert_eq!(
                    registration_no, no,
                    "timer job-control route changed registration signal"
                );
                assert!(is_job_control_signal(no));
                let opposite = if no != SigNo::SIGCONT {
                    SigSet::new_with_signos(&[SigNo::SIGCONT])
                } else {
                    SigSet::new_with_signos(&[
                        SigNo::SIGSTOP,
                        SigNo::SIGTSTP,
                        SigNo::SIGTTIN,
                        SigNo::SIGTTOU,
                    ])
                };
                let mut retired = inner.sig_pending.lock().flush_specific(opposite);
                for member in members.iter() {
                    retired.extend(member.sig_pending.lock().flush_specific(opposite));
                }

                if no == SigNo::SIGSTOP {
                    let transition = if self.tgid() == Tid::INIT {
                        crate::task::jobctl::group::JobControlTransition::NONE
                    } else {
                        let ThreadGroupInner {
                            members,
                            job_control,
                            ..
                        } = &mut *inner;
                        job_control
                            .as_mut()
                            .expect("jobctl: user ThreadGroup lacks control state")
                            .request_unconditional_stop(members, self.tgid(), no)
                    };
                    return (
                        (PosixTimerSignalEnqueue::Consumed, Vec::new(), no, retired),
                        transition,
                    );
                }

                let disposition_owner = route.private_target().unwrap_or_else(|| {
                    members
                        .first()
                        .expect("jobctl: live ThreadGroup has no disposition owner")
                });
                let disposition = disposition_owner.sig_disposition.read().get_disposition(no);
                let (discard, stop_epoch, transition) = if no == SigNo::SIGCONT {
                    let discard = disposition.action.is_explicit_ignore()
                        || disposition.action.is_default_ignore()
                            && !disposition_owner.is_current_sig_mask_blocking(no);
                    let transition = inner
                        .job_control
                        .as_mut()
                        .expect("jobctl: user ThreadGroup lacks control state")
                        .continue_generation(self.tgid());
                    (discard, None, transition)
                } else {
                    assert!(is_conditional_stop_signal(no));
                    let epoch = inner
                        .job_control
                        .as_ref()
                        .expect("jobctl: user ThreadGroup lacks control state")
                        .continue_epoch();
                    (
                        disposition.action.is_ignored(),
                        Some(epoch),
                        crate::task::jobctl::group::JobControlTransition::NONE,
                    )
                };
                let outcome = match route {
                    TimerJobControlSignalRoute::Shared => {
                        let mut pending = inner.sig_pending.lock();
                        registration.enqueue_job_control_locked(
                            &mut pending,
                            no,
                            generation,
                            episode,
                            overrun,
                            discard,
                            stop_epoch,
                        )
                    },
                    TimerJobControlSignalRoute::Private(target) => {
                        let mut pending = target.sig_pending.lock();
                        registration.enqueue_job_control_locked(
                            &mut pending,
                            no,
                            generation,
                            episode,
                            overrun,
                            discard,
                            stop_epoch,
                        )
                    },
                };
                let notify_targets = if matches!(outcome, PosixTimerSignalEnqueue::Queued) {
                    match route {
                        TimerJobControlSignalRoute::Shared => members.to_vec(),
                        TimerJobControlSignalRoute::Private(target) => vec![target.clone()],
                    }
                } else {
                    Vec::new()
                };
                ((outcome, notify_targets, no, retired), transition)
            })
        else {
            return PosixTimerSignalEnqueue::TargetExited;
        };
        let recheck_routes = if matches!(outcome, PosixTimerSignalEnqueue::Queued) {
            Some(self.snapshot_signalfd_rechecks())
        } else {
            None
        };

        // Pending publication precedes rearm; every target is armed before
        // job-control completion can wake a task or expose the new phase.
        for member in &notify_targets {
            member.rearm_signal_return_work();
        }
        self.finish_job_control_transition(transition);
        // Opposite-class cleanup spans shared and private owners. Each owner
        // extracts callbacks under its own leaf lock and completes them here.
        finish_timer_signal_flushes(retired);
        if let Some(routes) = recheck_routes {
            notify_signalfd_rechecks(routes);
        }
        for member in notify_targets {
            if !member.is_current_sig_mask_blocking(no) {
                notify(&member, false);
            }
        }
        outcome
    }

    /// Consume one live conditional default-stop occurrence. The occurrence
    /// was already claimed by Signal; this transaction only validates its
    /// narrow epoch authority and commits owner-local job-control effects.
    pub(in crate::task) fn request_conditional_job_control_stop(
        &self,
        task: &Arc<Task>,
        no: SigNo,
        expected_continue_epoch: ContinueEpoch,
    ) {
        assert!(is_conditional_stop_signal(no));
        let Some(((), transition)) = self.with_child_status_transaction(|members, inner| {
            let live_member = task.tgid() == self.tgid()
                && members.iter().any(|member| Arc::ptr_eq(member, task));
            if !matches!(inner.status.life_cycle(), ThreadGroupLifeCycle::Alive) || !live_member {
                return ((), crate::task::jobctl::group::JobControlTransition::NONE);
            }

            let ThreadGroupInner {
                members,
                job_control,
                ..
            } = inner;
            let transition = job_control
                .as_mut()
                .expect("jobctl: user ThreadGroup lacks control state")
                .request_conditional_stop(members, self.tgid(), no, expected_continue_epoch);
            ((), transition)
        }) else {
            return;
        };
        self.finish_job_control_transition(transition);
    }
}

/// Sender-specific control-generation authority and ordinary occurrence route.
///
/// Exact-member and process-group identities only validate the syscall target;
/// they are not job-control state. `SharedForExactMember` intentionally keeps
/// rt_sigqueueinfo's ordinary occurrence on the shared pending queue.
#[derive(Clone, Copy)]
enum JobControlSignalRoute<'a> {
    Shared,
    Private(&'a Arc<Task>),
    SharedForExactMember(&'a Arc<Task>),
    SharedForProcessGroup(Tid),
}

#[derive(Clone, Copy)]
enum TimerJobControlSignalRoute<'a> {
    Shared,
    Private(&'a Arc<Task>),
}

impl<'a> TimerJobControlSignalRoute<'a> {
    fn private_target(self) -> Option<&'a Arc<Task>> {
        match self {
            Self::Shared => None,
            Self::Private(target) => Some(target),
        }
    }
}

impl<'a> JobControlSignalRoute<'a> {
    fn exact_member(self) -> Option<&'a Arc<Task>> {
        match self {
            Self::Private(target) | Self::SharedForExactMember(target) => Some(target),
            Self::Shared | Self::SharedForProcessGroup(_) => None,
        }
    }

    fn expected_pgid(self) -> Option<Tid> {
        match self {
            Self::SharedForProcessGroup(pgid) => Some(pgid),
            Self::Shared | Self::Private(_) | Self::SharedForExactMember(_) => None,
        }
    }

    fn private_target(self) -> Option<&'a Arc<Task>> {
        match self {
            Self::Private(target) => Some(target),
            Self::Shared | Self::SharedForExactMember(_) | Self::SharedForProcessGroup(_) => None,
        }
    }
}
