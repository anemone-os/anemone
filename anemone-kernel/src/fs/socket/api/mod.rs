mod abi;
mod accept;
mod bind;
mod connect;
mod getpeername;
mod getsockname;
mod listen;
mod recvfrom;
mod sendto;
mod socket;
mod socketpair;

use crate::{
    fs::iomux::{IomuxScanOutcome, PollEvent, PollRegisterResult, wait_for_iomux_ready},
    prelude::*,
};

use super::SocketWait;

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

pub(super) fn wait_for_socket_operation(
    context: &'static str,
    task: &Arc<Task>,
    wait: &SocketWait,
    interests: PollEvent,
) -> Result<(), SysError> {
    wait_for_socket_source(context, task, interests, |request| wait.poll(request))
}
