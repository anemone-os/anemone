//! UDP-private Socket state and its static common-front operations.

mod datagram;
mod error;
mod source;

use crate::{
    net::udp::{UdpEndpointPort, create_endpoint},
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::{
    SocketCreation, SocketIoOps, SocketOps, SocketPreparation, SocketReleaseReason, SocketType,
};
use datagram::{
    bind_udp_socket, connect_udp_socket, query_udp_peer, query_udp_socket, receive_udp_socket,
    send_udp_socket, udp_is_accepting,
};
use error::{detach_udp_extended_error, mutate_udp_option, query_udp_option};
use source::UdpSocketSource;

#[derive(Opaque)]
struct UdpSocketFile {
    source: Arc<UdpSocketSource>,
    /// Serializes UDP state-changing operation attempts. It owns no Endpoint
    /// fact and is deliberately absent from final release.
    operation: Mutex<()>,
}

impl core::fmt::Debug for UdpSocketFile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UdpSocketFile").finish_non_exhaustive()
    }
}

impl UdpSocketFile {
    fn new(source: Arc<UdpSocketSource>) -> Self {
        Self {
            source,
            operation: Mutex::new(()),
        }
    }

    fn endpoint(&self) -> Option<UdpEndpointPort> {
        self.source.endpoint()
    }
}

/// Owns rollback authority until the common Socket description is published.
/// Capability clones never own semantic lifetime.
#[derive(Opaque)]
struct UdpSocketCreation {
    source: Option<Arc<UdpSocketSource>>,
}

impl UdpSocketCreation {
    fn commit(&mut self) {
        self.source.take();
    }
}

impl Drop for UdpSocketCreation {
    fn drop(&mut self) {
        let Some(source) = self.source.take() else {
            return;
        };
        let result = source.retire();
        assert!(
            result.is_ok(),
            "UDP socket creation rollback lost its endpoint identity"
        );
    }
}

fn udp_private(private: &AnyOpaque) -> &UdpSocketFile {
    private
        .cast::<UdpSocketFile>()
        .expect("UDP SocketOps used without UDP private state")
}

fn prepare_udp_socket() -> Result<SocketPreparation, SysError> {
    let endpoint = create_endpoint().map_err(|error| match error {
        anemone_net_api::udp::UdpCreateError::EndpointCapacity => SysError::NoBufferSpace,
    })?;
    let source = match UdpSocketSource::try_new(endpoint.clone()) {
        Ok(source) => source,
        Err(error) => {
            let retired = endpoint.retire();
            assert!(
                retired.is_ok(),
                "UDP source allocation rollback lost its Endpoint"
            );
            return Err(error);
        },
    };
    Ok(SocketPreparation {
        private: AnyOpaque::new(UdpSocketFile::new(source.clone())),
        creation: SocketCreation {
            commit: commit_udp_socket,
            authority: AnyOpaque::new(UdpSocketCreation {
                source: Some(source),
            }),
        },
    })
}

fn commit_udp_socket(creation: &mut AnyOpaque) {
    creation
        .cast_mut::<UdpSocketCreation>()
        .expect("UDP creation commit used without UDP creation authority")
        .commit();
}

fn poll_udp_socket(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    udp_private(private).source.poll(request)
}

fn final_release_udp_socket(private: &AnyOpaque, _reason: SocketReleaseReason) {
    // Source retirement first withdraws association, reverse publication and
    // routes. No sleeping operation mutex or fd-table lock participates.
    let result = udp_private(private).source.retire();
    assert!(
        result.is_ok(),
        "UDP final release lost its endpoint identity"
    );
}

pub(super) static UDP_SOCKET_OPS: SocketOps = SocketOps {
    io: SocketIoOps::Datagram {
        socket_type: SocketType::Ipv4Udp,
        send: send_udp_socket,
        receive: receive_udp_socket,
    },
    create: Some(prepare_udp_socket),
    create_pair: None,
    bind: Some(bind_udp_socket),
    listen: None,
    connect: Some(connect_udp_socket),
    accept: None,
    shutdown: None,
    local_address: Some(query_udp_socket),
    peer_address: Some(query_udp_peer),
    accepting: udp_is_accepting,
    query_option: Some(query_udp_option),
    mutate_option: Some(mutate_udp_option),
    detach_ipv4_extended_error: Some(detach_udp_extended_error),
    poll: poll_udp_socket,
    final_release: final_release_udp_socket,
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::{datagram::UdpSendSnapshot, *};

    use anemone_abi::fs::linux::{mode, statx};
    use anemone_net_api::{Ipv4Address, udp::UdpPeer};

    use crate::{
        fs::socket::{
            SocketAddress, SocketAddressSink, SocketDatagramSendOperation, SocketQueryError,
            SocketSendError, SocketSendPayload, SocketSendRequest, prepare_socket,
            socket_file_desc_ops, socket_from_file,
        },
        task::files::{OpenAccessMode, OpenedFileFinalReleaseCtx},
    };

    #[derive(Default)]
    struct AddressCapture(Option<SocketAddress>);

    impl SocketAddressSink for AddressCapture {
        fn copy_address(&mut self, address: Option<SocketAddress>) -> Result<(), SysError> {
            self.0 = address;
            Ok(())
        }
    }

    struct FaultSendPayload;

    impl SocketSendPayload for FaultSendPayload {
        fn bytes(&mut self, _maximum: usize) -> Result<&[u8], SysError> {
            Err(SysError::BadAddress)
        }
    }

    #[kunit]
    fn common_file_association_projects_udp_through_static_ops() {
        let (file, creation) =
            prepare_socket(&UDP_SOCKET_OPS).expect("KUnit UDP endpoint must fit");
        let socket = socket_from_file(&file).expect("prepared UDP file must be a Socket");
        let mut address = AddressCapture::default();
        socket.copy_local_address(&mut address).unwrap();
        assert_eq!(address.0, None);
        creation.commit();
        (socket_file_desc_ops().final_release.unwrap())(OpenedFileFinalReleaseCtx {
            file: &file,
            access: OpenAccessMode::ReadWrite,
            notification_suppressed: true,
        });
    }

    #[kunit]
    fn common_creation_guard_retires_udp_before_publication() {
        let (file, creation) =
            prepare_socket(&UDP_SOCKET_OPS).expect("KUnit UDP endpoint must fit");
        let socket = socket_from_file(&file).expect("prepared UDP file must be a Socket");
        drop(creation);
        assert_eq!(
            socket.copy_local_address(&mut AddressCapture::default()),
            Err(SocketQueryError::Retired)
        );
    }

    #[kunit]
    fn retry_keeps_connected_destination_snapshot_across_reconnect() {
        let (file, creation) =
            prepare_socket(&UDP_SOCKET_OPS).expect("KUnit UDP endpoint must fit");
        let socket = socket_from_file(&file).expect("prepared UDP file must be a Socket");
        let first_peer = Ipv4Address::LOOPBACK;
        let second_peer = Ipv4Address::new([127, 0, 0, 2]);
        assert!(
            socket
                .connect(SocketAddress::Ipv4 {
                    address: first_peer,
                    port: 7,
                })
                .is_ok()
        );

        let mut operation = SocketDatagramSendOperation::new();
        let mut payload = FaultSendPayload;
        assert_eq!(
            socket.send(SocketSendRequest::Datagram {
                destination: None,
                payload: &mut payload,
                operation: &mut operation,
            }),
            Err(SocketSendError::Copy(SysError::BadAddress))
        );
        assert_eq!(
            operation
                .family_snapshot::<UdpSendSnapshot>()
                .expect("first UDP send attempt must install a destination snapshot")
                .destination,
            UdpPeer::new(first_peer, 7)
        );

        assert!(
            socket
                .connect(SocketAddress::Ipv4 {
                    address: second_peer,
                    port: 9,
                })
                .is_ok()
        );
        assert_eq!(
            socket.send(SocketSendRequest::Datagram {
                destination: None,
                payload: &mut payload,
                operation: &mut operation,
            }),
            Err(SocketSendError::Copy(SysError::BadAddress))
        );
        assert_eq!(
            operation
                .family_snapshot::<UdpSendSnapshot>()
                .expect("retried UDP send must retain its first destination snapshot")
                .destination,
            UdpPeer::new(first_peer, 7)
        );

        drop(creation);
    }

    #[kunit]
    fn common_socket_inode_projects_linux_socket_type() {
        let (file, creation) =
            prepare_socket(&UDP_SOCKET_OPS).expect("KUnit UDP endpoint must fit");
        let attr = file
            .inode()
            .get_attr()
            .expect("Socket inode must report attrs");
        assert_eq!(attr.to_linux_stat().st_mode & mode::S_IFMT, mode::S_IFSOCK);
        assert_eq!(
            u32::from(attr.to_linux_statx(statx::BASIC_STATS).stx_mode) & mode::S_IFMT,
            mode::S_IFSOCK
        );
        drop(creation);
    }
}
