//! TCP Socket association, creation rollback, and final-release handoff.

use super::*;

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

pub(super) struct TcpSocketSource {
    /// The move-only Endpoint capability is the sole kernel association. The
    /// binding, role, peer, stream, cause, and readiness facts remain in the
    /// Stack owner and are queried for each operation.
    endpoint: SpinLock<Option<TcpEndpointPort>>,
}

impl TcpSocketSource {
    fn new(endpoint: TcpEndpointPort) -> Self {
        Self {
            endpoint: SpinLock::new(Some(endpoint)),
        }
    }

    pub(super) fn with_live<R>(&self, operation: impl FnOnce(&TcpEndpointPort) -> R) -> Option<R> {
        self.endpoint.lock().as_ref().map(operation)
    }

    fn release(&self, reason: TcpReleaseReason) -> bool {
        let endpoint = self.endpoint.lock().take();
        let Some(endpoint) = endpoint else {
            return false;
        };
        endpoint.release(reason);
        true
    }
}

impl Drop for TcpSocketSource {
    fn drop(&mut self) {
        let endpoint = self.endpoint.lock().take();
        if let Some(endpoint) = endpoint {
            // This is a fail-close assertion path, not a second lifecycle
            // trigger: withdraw and retire first so the diagnostic cannot leak
            // a live Endpoint if an unpublished/final-release owner is lost.
            endpoint.release(TcpReleaseReason::FinalRelease);
            panic!("TCP Socket source dropped before lifecycle-owned retirement");
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

pub(super) fn tcp_private_from_endpoint(endpoint: TcpEndpointPort) -> AnyOpaque {
    AnyOpaque::new(TcpSocketFile::new(Arc::new(TcpSocketSource::new(endpoint))))
}

pub(super) fn prepare_tcp_socket() -> Result<SocketPreparation, SysError> {
    let endpoint = create_endpoint().map_err(|error| match error {
        TcpCreateError::EndpointCapacity => SysError::NoBufferSpace,
    })?;
    let source = Arc::new(TcpSocketSource::new(endpoint));
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
