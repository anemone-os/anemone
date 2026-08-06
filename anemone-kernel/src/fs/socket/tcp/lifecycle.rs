//! TCP Socket association, creation rollback, and final-release handoff.

use super::*;

use crate::net::tcp::TcpEndpointPort;

#[derive(Opaque)]
pub(super) struct TcpSocketFile {
    pub(super) source: Arc<TcpSocketSource>,
    /// Serializes family operation attempts without owning any Stack fact.
    /// Final release deliberately bypasses this sleeping guard.
    pub(super) operation: Mutex<()>,
}

impl TcpSocketFile {
    fn new(source: Arc<TcpSocketSource>) -> Self {
        Self {
            source,
            operation: Mutex::new(()),
        }
    }
}

#[derive(Opaque)]
struct TcpSocketCreation {
    source: Option<Arc<TcpSocketSource>>,
}

impl TcpSocketCreation {
    fn commit(&mut self) {
        self.source.take();
    }
}

impl Drop for TcpSocketCreation {
    fn drop(&mut self) {
        let Some(source) = self.source.take() else {
            return;
        };
        assert!(
            source.release(TcpReleaseReason::CreationRollback),
            "TCP creation rollback lost its Endpoint capability"
        );
    }
}

pub(super) fn tcp_private(private: &AnyOpaque) -> &TcpSocketFile {
    private
        .cast::<TcpSocketFile>()
        .expect("TCP SocketOps used without TCP private state")
}

pub(super) fn tcp_private_from_accepted_endpoint(
    endpoint: TcpEndpointPort,
) -> Result<AnyOpaque, SysError> {
    finish_accepted_source(TcpSocketSource::try_new(endpoint))
}

fn finish_accepted_source(
    prepared: Result<Arc<TcpSocketSource>, (SysError, TcpEndpointPort)>,
) -> Result<AnyOpaque, SysError> {
    match prepared {
        Ok(source) => Ok(AnyOpaque::new(TcpSocketFile::new(source))),
        Err((error, endpoint)) => {
            endpoint.release(TcpReleaseReason::AcceptedChildRollback);
            Err(error)
        },
    }
}

pub(super) fn prepare_tcp_socket() -> Result<SocketPreparation, SysError> {
    let endpoint = create_endpoint().map_err(|error| match error {
        TcpCreateError::EndpointCapacity => SysError::NoBufferSpace,
    })?;
    let source = match TcpSocketSource::try_new(endpoint) {
        Ok(source) => source,
        Err((error, endpoint)) => {
            endpoint.release(TcpReleaseReason::CreationRollback);
            return Err(error);
        },
    };
    Ok(SocketPreparation {
        private: AnyOpaque::new(TcpSocketFile::new(source.clone())),
        creation: SocketCreation {
            commit: commit_tcp_socket,
            authority: AnyOpaque::new(TcpSocketCreation {
                source: Some(source),
            }),
        },
    })
}

fn commit_tcp_socket(creation: &mut AnyOpaque) {
    creation
        .cast_mut::<TcpSocketCreation>()
        .expect("TCP creation commit used without TCP creation authority")
        .commit();
}

pub(super) fn final_release_tcp_socket(private: &AnyOpaque, reason: SocketReleaseReason) {
    // Withdraw the only kernel Endpoint association without taking the
    // sleeping operation guard. Stack retirement and progression remain
    // non-blocking and infallible by the owner capability contract.
    assert!(
        tcp_private(private).source.release(match reason {
            SocketReleaseReason::AcceptedChildRollback => TcpReleaseReason::AcceptedChildRollback,
            SocketReleaseReason::FinalRelease => TcpReleaseReason::FinalRelease,
        }),
        "TCP final release lost its Endpoint capability"
    );
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use anemone_net_api::tcp::TcpQueryError;

    #[kunit]
    fn accepted_source_preparation_failure_uses_child_rollback_reason() {
        let endpoint = create_endpoint().expect("KUnit TCP Endpoint must fit");
        let access = endpoint.access();
        assert!(matches!(
            finish_accepted_source(Err((SysError::OutOfMemory, endpoint))),
            Err(SysError::OutOfMemory)
        ));
        assert_eq!(access.facts(), Err(TcpQueryError::UnknownEndpoint));
    }
}
