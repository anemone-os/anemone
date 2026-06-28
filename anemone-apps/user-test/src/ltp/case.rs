//! Single LTP case lifecycle.
//!
//! This module owns fork/exec, direct `wait4(child)`, soft per-case timeout,
//! and process-group kill fallback. It reports output and heartbeat events
//! through `LtpComponents`; it should not format judge-visible result lines
//! itself.

use anemone_rs::{
    os::linux::{
        fs::{AtFd, chdir, fstatat},
        process::{
            WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork, setpgid,
            signal::{SigNo, kill},
            wait4,
        },
    },
    prelude::*,
};

use super::{
    component::{
        LtpComponents,
        output_filter::LtpOutputFilter,
        wait_probe::{
            LtpWaitLoopProbe, elapsed_us_since_with_probe, now_us_with_probe,
            sched_yield_with_probe,
        },
    },
    config::{LtpGroup, LtpRoot, LtpRunPolicy},
    profile::LtpCaseSpec,
    result::{LTP_TCONF_EXIT_CODE, LtpCaseOutcome, ltp_exit_code},
};

enum LtpCaseWaitResult {
    Exited(WStatus),
    TimedOut,
}

pub(super) fn run_ltp_case(
    root: &LtpRoot,
    group: &LtpGroup,
    case: &LtpCaseSpec<'_>,
    case_path: &str,
    components: &mut LtpComponents,
    policy: &LtpRunPolicy,
) -> LtpCaseOutcome {
    components.on_case_start(root, group, case.name);
    let mut output_filter = LtpOutputFilter::start_or_disabled(case.name);

    match fork() {
        Ok(Some(tid)) => {
            output_filter.parent_after_fork();
            components.on_case_waiting(root, group, case.name, tid);
            match wait_ltp_case_status(
                tid,
                case.name,
                root.label,
                group.name,
                components,
                &mut output_filter,
                policy,
            ) {
                Ok(LtpCaseWaitResult::Exited(wstatus)) => {
                    output_filter.finish(tid, policy);
                    let exit_code = ltp_exit_code(wstatus);
                    let outcome = if exit_code == 0 {
                        LtpCaseOutcome::Passed
                    } else if exit_code == LTP_TCONF_EXIT_CODE {
                        LtpCaseOutcome::Skipped
                    } else {
                        LtpCaseOutcome::Failed
                    };
                    components.on_case_finished(root, group, case.name, tid, outcome, exit_code);
                    outcome
                },
                Ok(LtpCaseWaitResult::TimedOut) => {
                    output_filter.finish(tid, policy);
                    components.on_case_timeout(root, group, case.name, tid);
                    LtpCaseOutcome::InfraFailed
                },
                Err(errno) => {
                    output_filter.finish(tid, policy);
                    components.on_case_wait_failed(root, group, case.name, tid, errno);
                    LtpCaseOutcome::InfraFailed
                },
            }
        },
        Ok(None) => {
            if let Err(errno) = output_filter.child_attach() {
                components.child_attach_filter_failed(case.name, errno);
                exit(127);
            }

            // Fail closed if per-case process-group isolation cannot be
            // established. Running the testcase in the runner's process group
            // would preserve the original blast radius for kill(0) / kill(-pgid).
            if let Err(errno) = setpgid(0, 0) {
                components.child_setpgid_failed(case.name, errno);
                exit(127);
            }

            if let Err(errno) = chdir(root.workdir) {
                components.child_chdir_failed(case.name, root.workdir, errno);
                exit(127);
            }

            let mut argv = Vec::with_capacity(case.args.len() + 1);
            argv.push(case.executable);
            argv.extend(case.args.iter().copied());

            if let Err(errno) = execve(case_path, argv.as_slice(), root.envp) {
                components.child_exec_failed(case.name, case_path, errno);
            }
            exit(127);
        },
        Err(errno) => {
            components.on_case_fork_failed(root, group, case.name, errno);
            LtpCaseOutcome::InfraFailed
        },
    }
}

fn wait_ltp_case_status(
    tid: u32,
    name: &str,
    root_label: &str,
    group_name: &str,
    components: &mut LtpComponents,
    output_filter: &mut LtpOutputFilter,
    policy: &LtpRunPolicy,
) -> Result<LtpCaseWaitResult, Errno> {
    // Timeout is a user-test containment path, not a kernel watchdog contract:
    // after the soft deadline we kill the case pgrp, continue the profile, and
    // let the caller classify the outcome as infrastructure failure.
    let mut probe = LtpWaitLoopProbe::new(root_label, group_name, name, tid, policy);
    probe.begin_iteration();
    let start_us = now_us_with_probe(&mut probe)?;
    probe.finish_iteration();
    loop {
        output_filter.drain_available();
        probe.begin_iteration();
        if let Some(wstatus) = poll_ltp_case_status(tid, name, components, &probe)? {
            return Ok(LtpCaseWaitResult::Exited(wstatus));
        }

        if elapsed_us_since_with_probe(start_us, &mut probe)? >= policy.case_timeout_us {
            probe.finish_iteration();
            break;
        }

        sched_yield_with_probe(&probe)?;
        probe.finish_iteration();
    }

    components.case_timeout(name, policy.case_timeout_seconds);
    components.on_case_timeout_kill(root_label, group_name, name, tid);
    kill_ltp_case(tid, name, components)?;

    probe.set_phase("kill_grace");
    let kill_start_us = now_us_with_probe(&mut probe)?;
    loop {
        output_filter.drain_available();
        probe.begin_iteration();
        if poll_ltp_case_status(tid, name, components, &probe)?.is_some() {
            return Ok(LtpCaseWaitResult::TimedOut);
        }

        if elapsed_us_since_with_probe(kill_start_us, &mut probe)? >= policy.kill_grace_us {
            components.case_timeout_unreaped(name, policy.kill_grace_seconds);
            components.on_case_timeout_unreaped(root_label, group_name, name, tid);
            return Ok(LtpCaseWaitResult::TimedOut);
        }

        sched_yield_with_probe(&probe)?;
        probe.finish_iteration();
    }
}

fn poll_ltp_case_status(
    tid: u32,
    name: &str,
    components: &LtpComponents,
    probe: &LtpWaitLoopProbe<'_>,
) -> Result<Option<WStatus>, Errno> {
    let mut wstatus = WStatusRaw::EMPTY;
    probe.before("wait4");
    match wait4(
        WaitFor::ChildWithTgid(tid),
        Some(&mut wstatus),
        WaitOptions::NOHANG,
    ) {
        Ok(Some(waited)) => {
            probe.after("wait4", format_args!("waited={}", waited));
            if waited != tid {
                components.case_pid_mismatch(name);
                return Err(ECHILD);
            }
            Ok(Some(wstatus.read()))
        },
        Ok(None) => {
            probe.after("wait4", format_args!("none"));
            Ok(None)
        },
        Err(EINTR) => {
            probe.after("wait4", format_args!("eintr"));
            Ok(None)
        },
        Err(errno) => {
            probe.after("wait4", format_args!("err={errno:?}"));
            Err(errno)
        },
    }
}

fn kill_ltp_case(tid: u32, name: &str, components: &LtpComponents) -> Result<(), Errno> {
    let pid = i32::try_from(tid).map_err(|_| EINVAL)?;

    // The child calls setpgid(0, 0) before exec, so -pid normally reaches the
    // whole case tree. The direct-pid fallback only covers the pre-setpgid
    // window and is not a substitute for process-group cleanup.
    match kill(-pid, SigNo::SIGKILL) {
        Ok(()) => Ok(()),
        Err(ESRCH) => {
            components.case_pgrp_absent(name, tid);
            match kill(pid, SigNo::SIGKILL) {
                Ok(()) | Err(ESRCH) => Ok(()),
                Err(errno) => Err(errno),
            }
        },
        Err(errno) => Err(errno),
    }
}

pub(super) fn case_executable_exists(case_path: &str) -> bool {
    fstatat(AtFd::Cwd, Path::new(case_path)).is_ok()
}
