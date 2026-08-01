mod abi;
mod bind;
mod getsockname;
mod recvfrom;
mod sendto;
mod socket;

use crate::{
    fs::iomux::{IomuxScanOutcome, PollEvent, PollRegisterResult, wait_for_iomux_ready},
    prelude::*,
};

fn wait_for_socket_file(
    context: &'static str,
    task: &Arc<Task>,
    file: &File,
    interests: PollEvent,
) -> Result<(), SysError> {
    let outcome = wait_for_iomux_ready(context, task, None, |mode| {
        match file.poll(&mode.poll_request(interests))? {
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
