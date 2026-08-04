//! Family-neutral Socket operation retry and wait orchestration.

use crate::{
    fs::iomux::{IomuxScanOutcome, PollEvent, PollRegisterResult, wait_for_iomux_ready},
    prelude::*,
    task::sig::{
        SigNo, Signal,
        info::{SiCode, SigInfoFields, SigKill},
    },
};

use super::{SocketReceiveError, SocketReceiveOutcome, SocketSendError, SocketWait};

fn wait_for_socket_source(
    context: &'static str,
    task: &Arc<Task>,
    interests: PollEvent,
    mut poll: impl FnMut(&PollRequest<'_>) -> Result<PollRegisterResult, SysError>,
) -> Result<(), SysError> {
    let outcome = wait_for_iomux_ready(context, task, None, |mode| {
        match poll(&mode.poll_request(interests))? {
            PollRegisterResult::Subscribed(events) if mode.is_register() => Ok(
                IomuxScanOutcome::from_ready_count(usize::from(!events.is_empty())),
            ),
            PollRegisterResult::Subscribed(_) => {
                kwarningln!("{}: Socket snapshot unexpectedly subscribed", context);
                Err(SysError::IO)
            },
            PollRegisterResult::SubscribedRecheck if mode.is_register() => {
                Ok(IomuxScanOutcome::Recheck)
            },
            PollRegisterResult::SubscribedRecheck => {
                kwarningln!(
                    "{}: Socket snapshot unexpectedly requested recheck",
                    context
                );
                Err(SysError::IO)
            },
            PollRegisterResult::Ready(events) if !events.is_empty() => {
                Ok(IomuxScanOutcome::Ready(1))
            },
            PollRegisterResult::Ready(_) if mode.is_register() => {
                kwarningln!("{}: Socket register scan returned empty ready", context);
                Ok(IomuxScanOutcome::Unsupported)
            },
            PollRegisterResult::Ready(_) => Ok(IomuxScanOutcome::NotReady),
            PollRegisterResult::Unsupported => Ok(IomuxScanOutcome::Unsupported),
        }
    });
    let ready = outcome.into_result_without_temporary_mask()?;
    assert_eq!(
        ready, 1,
        "Socket wait returned without its single source ready"
    );
    Ok(())
}

pub(super) fn wait_for_socket_file(
    context: &'static str,
    task: &Arc<Task>,
    file: &File,
    interests: PollEvent,
) -> Result<(), SysError> {
    wait_for_socket_source(context, task, interests, |request| file.poll(request))
}

pub(in crate::fs::socket) fn wait_for_socket_operation(
    context: &'static str,
    task: &Arc<Task>,
    wait: &SocketWait,
    interests: PollEvent,
) -> Result<(), SysError> {
    wait_for_socket_source(context, task, interests, |request| wait.poll(request))
}

fn send_sigpipe(task: &Arc<Task>) {
    task.recv_signal(Signal::new(
        SigNo::SIGPIPE,
        SiCode::Kernel,
        SigInfoFields::Kill(SigKill {
            pid: task.tgid(),
            uid: task.cred().uid.real,
        }),
    ));
}

/// Retries a family-owned send attempt after its owner-defined predicate.
/// Record-oriented families may provide an operation-specific wait whose
/// predicate includes the complete payload admission requirement; otherwise
/// the file's public writability predicate is used. A `WouldBlock` result must
/// not retain a family operation guard, and neither wait carries readiness
/// truth across the recheck.
pub(in crate::fs::socket) fn retry_socket_send(
    context: &'static str,
    task: &Arc<Task>,
    file: &File,
    operation_wait: Option<&SocketWait>,
    nonblocking: bool,
    raise_sigpipe_on_peer_close: bool,
    mut attempt: impl FnMut() -> Result<usize, SocketSendError>,
    map_error: impl Fn(SocketSendError) -> SysError,
) -> Result<usize, SysError> {
    loop {
        match attempt() {
            Ok(sent) => return Ok(sent),
            Err(SocketSendError::WouldBlock) if !nonblocking => {
                if let Some(wait) = operation_wait {
                    wait_for_socket_operation(context, task, wait, PollEvent::WRITABLE)?;
                } else {
                    wait_for_socket_file(context, task, file, PollEvent::WRITABLE)?;
                }
            },
            Err(SocketSendError::PeerClosed) => {
                if raise_sigpipe_on_peer_close {
                    send_sigpipe(task);
                }
                return Err(map_error(SocketSendError::PeerClosed));
            },
            Err(error) => return Err(map_error(error)),
        }
    }
}

/// Retries a family-owned receive attempt after the file's owner-defined
/// readability predicate. A `WouldBlock` result must not retain a family
/// operation guard; this driver carries no family state across the wait.
pub(in crate::fs::socket) fn retry_socket_receive(
    context: &'static str,
    task: &Arc<Task>,
    file: &File,
    nonblocking: bool,
    mut attempt: impl FnMut() -> Result<SocketReceiveOutcome, SocketReceiveError>,
    map_error: impl Fn(SocketReceiveError) -> SysError,
) -> Result<SocketReceiveOutcome, SysError> {
    loop {
        match attempt() {
            Ok(received) => return Ok(received),
            Err(SocketReceiveError::WouldBlock) if !nonblocking => {
                wait_for_socket_file(context, task, file, PollEvent::READABLE)?;
            },
            Err(error) => return Err(map_error(error)),
        }
    }
}
