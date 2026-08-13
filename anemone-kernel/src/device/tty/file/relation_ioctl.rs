use crate::prelude::*;

use super::{super::relation, TtyFile, TtyOperation, begin_external_effect, read_ioctl_value};

fn current_tty_caller() -> Result<crate::task::jobctl::TtyCaller, SysError> {
    crate::task::jobctl::TtyCaller::current().map_err(|_| SysError::UnsupportedIoctl)
}

fn controlling_snapshot(
    tty: &TtyFile,
    caller: &crate::task::jobctl::TtyCaller,
) -> Result<relation::RelationSnapshot, SysError> {
    let snapshot = relation::endpoint_snapshot(&tty.endpoint).ok_or(SysError::UnsupportedIoctl)?;
    if !snapshot.session().same_identity(caller.session()) {
        return Err(SysError::UnsupportedIoctl);
    }
    Ok(snapshot)
}

pub(super) fn set_controlling_tty(tty: &TtyFile, ctx: &IoctlCtx<'_>) -> Result<(), SysError> {
    // Privileged stealing (`arg=1`) is outside the accepted first-version ABI;
    // rejecting every nonzero value avoids a silent success with weaker effect.
    if ctx.arg() != 0 {
        knoticeln!(
            "TTY: rejecting unsupported TIOCSCTTY steal argument {}",
            ctx.arg()
        );
        return Err(SysError::PermissionDenied);
    }
    let caller = current_tty_caller()?;
    relation::acquire(&tty.endpoint, &caller, ctx.target_access().can_read())
}

pub(super) fn detach_controlling_tty(tty: &TtyFile) -> Result<(), SysError> {
    relation::detach(&tty.endpoint, &current_tty_caller()?)
}

pub(super) fn controlling_sid(tty: &TtyFile) -> Result<i32, SysError> {
    loop {
        let caller = current_tty_caller()?;
        let snapshot = controlling_snapshot(tty, &caller)?;
        if caller.revalidate() && snapshot.is_current() {
            return Ok(snapshot.session().sid().get() as i32);
        }
    }
}

pub(super) fn foreground_pgid(tty: &TtyFile) -> Result<i32, SysError> {
    loop {
        let caller = current_tty_caller()?;
        let snapshot = controlling_snapshot(tty, &caller)?;
        let pgid = snapshot
            .foreground()
            .map_or(0, |foreground| foreground.pgid().get() as i32);
        if caller.revalidate() && snapshot.is_current() {
            return Ok(pgid);
        }
    }
}

fn begin_set_foreground_effect(
    operation: Option<&dyn TtyOperation>,
) -> Result<Option<super::PtyEffectPermit>, SysError> {
    begin_external_effect(operation).map_err(|_| SysError::UnsupportedIoctl)
}

pub(super) fn set_foreground_pgid(
    tty: &TtyFile,
    operation: Option<&dyn TtyOperation>,
    ctx: &IoctlCtx<'_>,
) -> Result<(), SysError> {
    loop {
        let caller = current_tty_caller()?;
        let snapshot = controlling_snapshot(tty, &caller)?;

        // POSIX terminal access checks precede touching the user candidate. An
        // actionable background SIGTTOU therefore has no foreground mutation
        // and restarts this idempotent ioctl only after signal handling.
        if matches!(
            caller.sigttou_decision(snapshot.foreground()),
            crate::task::jobctl::TtySigttouDecision::Signal
        ) {
            let _effect = begin_set_foreground_effect(operation)?;
            if !caller.revalidate() || !snapshot.is_current() {
                continue;
            }
            if caller.signal_process_group_sigttou() {
                return Err(SysError::RestartSyscall(RestartSyscall::Idempotent));
            }
            continue;
        }

        // Preserve the POSIX access-check ordering above, but keep user-memory
        // access outside the PTY permit that master final release must drain.
        let raw_pgid = read_ioctl_value::<i32>(ctx)?;
        if raw_pgid < 0 {
            return Err(SysError::InvalidArgument);
        }
        let foreground = caller.resolve_process_group(Tid::new(raw_pgid as u32))?;
        let _effect = begin_set_foreground_effect(operation)?;
        if !caller.revalidate()
            || !foreground.is_live_in(caller.session())
            || !snapshot.is_current()
        {
            continue;
        }
        let pgid = foreground.pgid();
        if relation::commit_foreground(&snapshot, foreground) {
            kdebugln!(
                "TTY: foreground commit sid={} pgid={}",
                caller.session().sid(),
                pgid
            );
            return Ok(());
        }
    }
}
