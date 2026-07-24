use anemone_abi::process::linux::{signal as linux_signal, ucontext::UContext};

use crate::{
    prelude::*,
    sched::CurrentWaitOutcome,
    syscall::user_access::UserWritePtr,
    task::{
        Task,
        exit::{kernel_exit, kernel_exit_group},
        jobctl::UserEntryOutcome,
        sig::{
            disposition::{KSigAction, SaFlags, SignalAction},
            set::SigSet,
        },
    },
};

use super::{
    RtSigFrame, SigNo, Signal, SignalArchTrait, disposition::SignalDisposition,
    pending::FetchedSignal,
};

/// Typed wait outcome candidate for delayed temporary-mask classification.
///
/// This is intentionally signal-owned. Delayed-restore callsites should pass
/// their scheduler/latch outcome through this type instead of interpreting
/// pending queues, dispositions, ignore/default/custom actions, or force wakes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporaryMaskWaitCandidate {
    PredicateReady,
    Timeout,
    Signal,
    Force,
    Cancelled,
    Unexpected,
}

impl From<CurrentWaitOutcome> for TemporaryMaskWaitCandidate {
    fn from(value: CurrentWaitOutcome) -> Self {
        match value {
            CurrentWaitOutcome::PredicateReady => Self::PredicateReady,
            CurrentWaitOutcome::Timeout => Self::Timeout,
            CurrentWaitOutcome::Signal => Self::Signal,
            CurrentWaitOutcome::Force => Self::Force,
            CurrentWaitOutcome::Cancelled => Self::Cancelled,
            CurrentWaitOutcome::Unexpected => Self::Unexpected,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporaryMaskWaitContext {
    RtSigsuspend,
    Ppoll,
    Pselect6,
}

impl TemporaryMaskWaitContext {
    const fn as_str(self) -> &'static str {
        match self {
            Self::RtSigsuspend => "rt_sigsuspend",
            Self::Ppoll => "ppoll",
            Self::Pselect6 => "pselect6",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporaryMaskWaitReturn {
    /// The wait was not a signal-delivery carrier; restore the token and let
    /// the caller map its original typed wait result.
    OriginalOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporaryMaskWaitDecision {
    /// A stable trap-return delivery target is already reserved for this task.
    DeferToTrapReturnDelivery,
    RestoreThenReturn(TemporaryMaskWaitReturn),
    RestoreThenFailClosed(SysError),
    /// A force wake matched a non-returning signal target. Callers must not map
    /// this to an ordinary `EINTR` carrier.
    NoReturnForce,
}

impl Task {
    /// Check whether this task has unmasked signals.
    ///
    /// 'masked' here refers to the signal mask of this task.
    ///
    /// This method relies on the fact that ignored signals won't be delivered
    /// into [PendingSignals]. See [Task::recv_signal] for details.
    pub fn has_unmasked_signal(&self) -> bool {
        let prv_pending = {
            let pending = self.sig_pending.lock();
            let mask = self.snapshot_current_sig_mask();
            pending.has_unmasked(mask)
        };
        if prv_pending {
            return true;
        }

        // here is a window. does this matter? i think not. but im not sure.

        let tg = self.get_thread_group();
        let tg_inner = tg.inner.read();
        let shared_pending = {
            let pending = tg_inner.sig_pending.lock();
            let mask = self.snapshot_current_sig_mask();
            pending.has_unmasked(mask)
        };
        shared_pending
    }

    pub fn has_specific_signal(&self, set: SigSet) -> bool {
        {
            let pending = self.sig_pending.lock();
            if pending.has_specific(set) {
                return true;
            }
        }
        {
            let tg = self.get_thread_group();
            let tg_inner = tg.inner.read();
            let pending = tg_inner.sig_pending.lock();
            if pending.has_specific(set) {
                return true;
            }
        }
        false
    }
    /// Called when this task is about to return to user-space.
    ///
    /// Masked signals won't be fetched.
    ///
    /// See [PendingSignals::fetch_any] for the order of fetching signals.
    fn fetch_signal(&self) -> Option<FetchedSignal> {
        let tg = self.get_thread_group();
        let tg_inner = tg.inner.read();
        let phase = match tg_inner.status.life_cycle() {
            ThreadGroupLifeCycle::Alive => {
                if tg_inner
                    .job_control
                    .as_ref()
                    .expect("jobctl: user ThreadGroup lacks control state")
                    .is_running()
                {
                    SignalFetchPhase::Running
                } else {
                    SignalFetchPhase::Stopped
                }
            },
            ThreadGroupLifeCycle::Exiting(_) | ThreadGroupLifeCycle::Exited(_) => return None,
        };

        // The ThreadGroup owner read guard serializes phase selection with
        // control generation. Pending claim then follows the established
        // ThreadGroup -> Signal-leaf direction.
        {
            let mut pending = self.sig_pending.lock();
            let mask = self.snapshot_current_sig_mask();
            let dispositions = self.sig_disposition.read();
            if let Some(signal) = pending.fetch_matching(mask, |signal, reserved| {
                phase.allows(signal, reserved, &dispositions)
            }) {
                return Some(signal);
            }
        }

        let mut pending = tg_inner.sig_pending.lock();
        let mask = self.snapshot_current_sig_mask();
        let dispositions = self.sig_disposition.read();
        pending.fetch_matching(mask, |signal, reserved| {
            assert!(!reserved, "shared pending cannot contain a reservation");
            phase.allows(signal, false, &dispositions)
        })
    }

    /// See [PendingSignals::fetch_specific] for more details.
    pub fn fetch_specific_signal(&self, set: SigSet) -> Option<Signal> {
        // first private pending
        {
            let mut pending = self.sig_pending.lock();
            if let Some(signal) = pending.fetch_specific(set) {
                return Some(signal);
            }
        }

        // no private signals satisfied the criteria. check shared pending signals.
        {
            let tg = self.get_thread_group();
            let tg_inner = tg.inner.read();
            let mut pending = tg_inner.sig_pending.lock();
            if let Some(signal) = pending.fetch_specific(set) {
                return Some(signal);
            }
        }

        None
    }

    pub fn classify_temporary_mask_wait(
        &self,
        candidate: impl Into<TemporaryMaskWaitCandidate>,
        context: TemporaryMaskWaitContext,
    ) -> TemporaryMaskWaitDecision {
        let candidate = candidate.into();
        match candidate {
            TemporaryMaskWaitCandidate::PredicateReady | TemporaryMaskWaitCandidate::Timeout => {
                TemporaryMaskWaitDecision::RestoreThenReturn(
                    TemporaryMaskWaitReturn::OriginalOutcome,
                )
            },
            TemporaryMaskWaitCandidate::Cancelled | TemporaryMaskWaitCandidate::Unexpected => {
                self.log_temporary_mask_classifier_fail_closed(candidate, context);
                TemporaryMaskWaitDecision::RestoreThenFailClosed(SysError::IO)
            },
            TemporaryMaskWaitCandidate::Signal | TemporaryMaskWaitCandidate::Force => {
                self.classify_signal_temporary_mask_candidate(candidate, context)
            },
        }
    }

    fn classify_signal_temporary_mask_candidate(
        &self,
        candidate: TemporaryMaskWaitCandidate,
        context: TemporaryMaskWaitContext,
    ) -> TemporaryMaskWaitDecision {
        let force_only = matches!(candidate, TemporaryMaskWaitCandidate::Force);
        let Some(signo) = self.reserve_temporary_mask_delivery_target(force_only) else {
            self.log_temporary_mask_classifier_fail_closed(candidate, context);
            return TemporaryMaskWaitDecision::RestoreThenFailClosed(SysError::IO);
        };

        let action = self.sig_disposition.read().get_disposition(signo).action;
        kdebugln!(
            "{}: temporary-mask classifier reserved task={} candidate={:?} signo={:?} action={:?}",
            context.as_str(),
            self.tid(),
            candidate,
            signo,
            action,
        );

        if matches!(signo, SigNo::SIGKILL | SigNo::SIGSTOP) {
            // Force-wake targets are deliberately not represented as an
            // ordinary EINTR carrier. The signal is reserved for trap return;
            // the callsite must restore the token and enter its no-return
            // force path instead of treating the wait as a recoverable signal.
            return TemporaryMaskWaitDecision::NoReturnForce;
        }

        if force_only {
            self.log_temporary_mask_classifier_fail_closed(candidate, context);
            return TemporaryMaskWaitDecision::RestoreThenFailClosed(SysError::IO);
        }

        TemporaryMaskWaitDecision::DeferToTrapReturnDelivery
    }

    fn reserve_temporary_mask_delivery_target(&self, force_only: bool) -> Option<SigNo> {
        if let Some(signo) = self.private_reserved_delivery_signo() {
            return Some(signo);
        }

        if let Some(signo) = self.reserve_private_temporary_mask_delivery_target(force_only) {
            return Some(signo);
        }

        self.reserve_shared_temporary_mask_delivery_target(force_only)
    }

    fn private_reserved_delivery_signo(&self) -> Option<SigNo> {
        self.sig_pending.lock().reserved_delivery_signo()
    }

    fn reserve_private_temporary_mask_delivery_target(&self, force_only: bool) -> Option<SigNo> {
        let mut pending = self.sig_pending.lock();
        if let Some(signo) = pending.reserved_delivery_signo() {
            return Some(signo);
        }

        let reserved = if force_only {
            pending.reserve_specific_for_delivery(force_signal_set())
        } else {
            let mask = self.sig_mask.lock().current();
            pending.reserve_any_for_delivery(mask)
        };

        reserved.then(|| {
            pending
                .reserved_delivery_signo()
                .expect("reserved temporary delivery target must record a signal number")
        })
    }

    fn reserve_shared_temporary_mask_delivery_target(&self, force_only: bool) -> Option<SigNo> {
        let signal = {
            let tg = self.get_thread_group();
            let tg_inner = tg.inner.read();
            let mut pending = tg_inner.sig_pending.lock();
            if force_only {
                pending.fetch_specific(force_signal_set())
            } else {
                let mask = self.sig_mask.lock().current();
                pending.fetch_unreserved_any(mask)
            }
        }?;

        let mut pending = self.sig_pending.lock();
        Some(pending.reserve_delivery_target(signal))
    }

    fn log_temporary_mask_classifier_fail_closed(
        &self,
        candidate: TemporaryMaskWaitCandidate,
        context: TemporaryMaskWaitContext,
    ) {
        kwarningln!(
            "{}: temporary-mask classifier fail-closed task={} candidate={:?} current_mask={:?} private_pending={:?} shared_pending={:?}",
            context.as_str(),
            self.tid(),
            candidate,
            self.snapshot_current_sig_mask(),
            self.pending_signal_set(),
            self.get_thread_group().shared_pending_signal_set(),
        );
    }
}

#[derive(Clone, Copy)]
enum SignalFetchPhase {
    Running,
    Stopped,
}

impl SignalFetchPhase {
    fn allows(self, signal: &Signal, reserved: bool, dispositions: &SignalDisposition) -> bool {
        if matches!(self, Self::Running) {
            return true;
        }
        if signal.no == SigNo::SIGKILL || reserved && signal.no == SigNo::SIGCONT {
            return true;
        }

        let action = dispositions.get_disposition(signal.no).action;
        if matches!(signal.no, SigNo::SIGTSTP | SigNo::SIGTTIN | SigNo::SIGTTOU)
            && signal.default_stop_epoch().is_some()
            && action.is_default_stop()
        {
            return true;
        }

        signal.is_kernel_synchronous_fault() && action.is_default_terminal()
    }
}

fn force_signal_set() -> SigSet {
    SigSet::new_with_signos(&[SigNo::SIGKILL, SigNo::SIGSTOP])
}

/// Complete Signal/lifecycle/jobctl arbitration immediately before a user
/// transition. Returns with interrupts disabled and exposure registered.
pub(crate) fn arbitrate_user_entry(
    trapframe: &mut TrapFrame,
    mut restart_syscall: Option<(RestartSyscall, SyscallCtx)>,
) {
    let mut restart_crossed_jobctl_park = false;
    loop {
        if !IntrArch::local_intr_enabled() {
            // Fresh, clone, and exec entries arrive from the scheduler with
            // interrupts disabled. They still need the same first Signal pass
            // as an ordinary trap return before the final noirq gate.
            unsafe {
                IntrArch::local_intr_enable();
            }
        }
        handle_signals(trapframe, &mut restart_syscall);
        unsafe {
            IntrArch::local_intr_disable();
        }

        match get_current_task().before_user_entry() {
            UserEntryOutcome::Admitted => {
                // A default job-control stop has no user handler that can own
                // syscall restart. Keep the trap-local capability across the
                // mandatory park, then restore it only after Signal has
                // rescanned the resume path and the final gate admits user
                // execution. Ordinary no-handler signal paths retain their
                // existing EINTR behavior because they never crossed a park.
                if restart_crossed_jobctl_park {
                    if let Some((restart, syscall_ctx)) = restart_syscall.take() {
                        match restart {
                            RestartSyscall::Idempotent => {
                                kdebugln!(
                                    "restarting syscall after job-control park: sysno = {}",
                                    syscall_ctx.syscall_no()
                                );
                                TrapArch::restore_syscall_ctx(trapframe, &syscall_ctx);
                            },
                        }
                    }
                }
                return;
            },
            UserEntryOutcome::Recheck => {
                restart_crossed_jobctl_park |= restart_syscall.is_some();
            },
            UserEntryOutcome::Exit(code) => {
                // User-entry exclusion is decided atomically under the owner
                // with interrupts disabled, but lifecycle teardown may close
                // files and wake waiters and therefore must remain sleepable.
                unsafe {
                    IntrArch::local_intr_enable();
                }
                kernel_exit(code)
            },
            UserEntryOutcome::Park => {
                unreachable!("jobctl park must resolve before returning")
            },
        }

        // A park wake is only a rescan opportunity. Signal handling owns the
        // next interrupt-enabled pass; the final gate always runs with
        // interrupts disabled before FPU ownership is restored.
    }
}

/// **Called by trap handling code when returning to user-space.**
///
/// Internally a loop for handling pending signals.
///
/// Since we support kernel preemption, it seems no need to limit how many
/// signals we handle in one go? idk.
pub fn handle_signals(
    trapframe: &mut TrapFrame,
    restart_syscall: &mut Option<(RestartSyscall, SyscallCtx)>,
) {
    let mut committed_handler_frame = false;
    loop {
        // Keep the current-task Arc out of `perform_signal_action()`: a default
        // action may terminate without returning, and an if-let scrutinee
        // temporary would then remain forever on the exiting task's own stack.
        let fetched = {
            let task = get_current_task();
            task.fetch_signal()
        };
        if let Some(FetchedSignal { signal, reserved }) = fetched {
            match perform_signal_action(signal, trapframe, restart_syscall) {
                SignalActionResult::Continue => {
                    if reserved {
                        // Reservation retirement ends this ordinary scan even
                        // when live action selection produced no handler frame.
                        // Signal remains the sole owner of temporary-mask
                        // cleanup below.
                        break;
                    }
                },
                SignalActionResult::HandlerFrame => {
                    committed_handler_frame = true;
                    break;
                },
                SignalActionResult::EndScanNoFrame => break,
            }
        } else {
            break;
        }
    }

    // Ignored signals do not leave an rt_sigreturn frame behind. If this
    // trap-return pass consumed no handler signal, signal code remains the
    // owner responsible for closing any deferred temporary-mask restore.
    if !committed_handler_frame {
        get_current_task().restore_temporary_sig_mask_if_pending();
    }
}

/// Report whether action selection may continue scanning, committed a handler
/// frame, or consumed an action that must hand control back to the entry gate.
enum SignalActionResult {
    Continue,
    HandlerFrame,
    EndScanNoFrame,
}

fn perform_signal_action(
    signal: Signal,
    trapframe: &mut TrapFrame,
    restart_syscall: &mut Option<(RestartSyscall, SyscallCtx)>,
) -> SignalActionResult {
    if signal.is_dethread_victim_kill() {
        assert_eq!(signal.no, SigNo::SIGKILL);
        let task = get_current_task();
        let ordinary_sigkill_pending = task.sig_pending.lock().take_ordinary_sigkill();
        // A temporary-mask force wake may have reserved this internal kill.
        // No signal frame or rt_sigreturn path remains after victim teardown.
        task.restore_temporary_sig_mask_if_pending();
        drop(task);
        if ordinary_sigkill_pending {
            kernel_exit_group(ExitCode::Signaled(SigNo::SIGKILL))
        }
        kernel_exit(ExitCode::Exited(0))
    }

    let no = signal.no;
    let task = get_current_task();
    let KSigAction {
        action,
        flags,
        restorer,
        mask,
    } = task.sig_disposition.read().get_disposition(no);

    match action {
        SignalAction::Default(default) => {
            if action.is_default_stop() {
                let expected_continue_epoch = signal.default_stop_epoch().unwrap_or_else(|| {
                    panic!(
                        "jobctl: default-stop signal {:?} lacks conditional authority",
                        no
                    )
                });
                task.get_thread_group()
                    .request_conditional_job_control_stop(&task, no, expected_continue_epoch);
                return SignalActionResult::EndScanNoFrame;
            } else {
                if action.is_default_terminal() {
                    // A reserved occurrence may select a terminal action after
                    // temporary-mask defer. No trap-return cleanup will run on
                    // this path, so Signal must retire the restore slot before
                    // handing control to lifecycle teardown.
                    task.restore_temporary_sig_mask_if_pending();
                }
                drop(task);
                default(no);
            }
            return SignalActionResult::Continue;
        },
        SignalAction::Ignore => {
            return SignalActionResult::Continue;
        },
        SignalAction::Custom(handler_addr) => {
            if !flags.contains(SaFlags::RESTART) {
                // A live custom action without SA_RESTART converts any
                // syscall-restart request into the user-visible EINTR already
                // stored in the trapframe, including after a job-control park.
                restart_syscall.take();
            }
            let mask_to_save = task.sigmask_to_save_for_signal_frame();
            task.mutate_current_sig_mask_for_signal_delivery(|sig_mask| {
                assert!(
                    !mask.get(SigNo::SIGKILL) && !mask.get(SigNo::SIGSTOP),
                    "SIGKILL and SIGSTOP cannot be masked"
                );
                sig_mask.union_with(&mask);
                if !flags.contains(SaFlags::NODEFER) {
                    sig_mask.set(no);
                }
            });

            // if SA_ONSTACK is set, and altstack is configured:
            // - if we're not currently on the altstack, use the altstack.
            // - if we're already on the altstack, we have a reentrancy. we just push
            //   sigframe onto currently-using stack.

            let (altstack, init_sp) = {
                let curr_sp = trapframe.sp();
                let altstack = *task.sig_altstack.lock();
                if let Some(altstack) = altstack {
                    let mut ss = altstack.to_linux_sigstack();
                    if flags.contains(SaFlags::ONSTACK) {
                        if altstack.contains_addr(VirtAddr::new(curr_sp)) {
                            // we're already on altstack. this is a reentrancy. continue to use this
                            // altstack.
                            ss.ss_flags |= linux_signal::SS_ONSTACK;
                            (ss, curr_sp)
                        } else {
                            // first time to use altstack.
                            (ss, ss.ss_sp as u64 + ss.ss_size as u64)
                        }
                    } else {
                        // SA_ONSTACK is not set but altstack is configured.
                        (ss, curr_sp)
                    }
                } else {
                    // altstack not configured. just use current stack.
                    (
                        linux_signal::SigStack {
                            ss_sp: 0 as *mut u8,
                            ss_flags: linux_signal::SS_DISABLE,
                            ss_size: 0,
                        },
                        curr_sp,
                    )
                }
            };

            let mut ucontext = UContext::ZEROED;

            if flags.contains(SaFlags::RESTART) {
                // note the take. this ensures only the first signal handler with SA_RESTART can
                // restart the syscall.
                if let Some((restart, syscall_ctx)) = restart_syscall.take() {
                    match restart {
                        RestartSyscall::Idempotent => {
                            kdebugln!("restarting syscall: sysno = {}", syscall_ctx.syscall_no());

                            TrapArch::restore_syscall_ctx(trapframe, &syscall_ctx);
                            // arguments are still in registers, and will be
                            // encoded into ucontext.
                        },
                    }
                }

                // this must be done before encoding ucontext.

                // no need to set break_loop, since all user-defined handlers
                // will set that anyway.
            }

            SignalArch::encode_ucontext(
                &mut ucontext,
                trapframe,
                mask_to_save,
                altstack,
                task.fpu_used(),
            );

            // construct signal frame on user stack.
            let frame = RtSigFrame {
                siginfo: signal.to_linux_siginfo(),
                ucontext,
            };

            let sigframe_base = VirtAddr::new(align_down_power_of_2!(
                init_sp - size_of::<RtSigFrame>() as u64,
                // 16 bytes should be enough for all architectures.
                16
            ) as u64);
            let write_sigframe = {
                let usp = task.clone_uspace_handle();
                let mut guard = usp.lock();
                match UserWritePtr::<RtSigFrame>::try_new(sigframe_base, &mut guard) {
                    Ok(mut uptr) => {
                        uptr.write(frame);
                        Ok(())
                    },
                    Err(e) => Err(e),
                }
            };
            if let Err(e) = write_sigframe {
                knoticeln!(
                    "perform_signal_action: failed to write sigframe to {} user stack: {:?}",
                    task.tid(),
                    e
                );
                // Frame construction failed after Signal took ownership of a
                // possible deferred restore. The terminal path never reaches
                // handle_signals() no-frame cleanup or rt_sigreturn().
                task.restore_temporary_sig_mask_if_pending();
                kernel_exit_group(ExitCode::Signaled(SigNo::SIGSEGV))
            }

            // we're done. finally prepare the trapframe.
            SignalArch::prepare_trapframe_for_signal_handler(
                trapframe,
                no,
                handler_addr,
                sigframe_base,
            );

            // place this after preparing trapframe for overwriting.
            if flags.contains(SaFlags::RESTORER) {
                trapframe.set_return_addr(restorer.get());
            }

            task.signal_frame_committed_restore_mask();
        },
    }

    if flags.contains(SaFlags::ONESHOT) {
        task.sig_disposition.write().set_to_default(no);
    }

    SignalActionResult::HandlerFrame
}
