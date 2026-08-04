//! ICMP raw family-private Socket state and static common-front operations.
//!
//! The static descriptor is the only Linux raw-ICMP semantic type witness;
//! family state stays private behind the common Socket front.

mod source;

use anemone_net_api::{
    Ipv4Address,
    icmp_raw::{
        IcmpRawEgressPolicy, IcmpRawMutationError, IcmpRawQueryError, IcmpRawReceiveError,
        IcmpRawReceivedPacket, IcmpRawSendError, IcmpRawTypeFilter,
    },
};

use crate::{
    kconfig_defs::{NET_ICMP_RAW_DEFAULT_TOS, NET_ICMP_RAW_DEFAULT_TTL},
    net::icmp_raw::{
        BindError, ConnectError, IcmpRawEndpointPort, IcmpRawSendSelection, SendError,
        create_endpoint,
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::{
    SocketAddress, SocketAddressSink, SocketBindError, SocketConnectError, SocketCreation,
    SocketDatagramSendOperation, SocketOps, SocketOptionError, SocketOptionMutation,
    SocketOptionQuery, SocketOptionValue, SocketPayloadIo, SocketPreparation, SocketQueryError,
    SocketReceiveError, SocketReceiveOutcome, SocketReceiveRequest, SocketSendError,
    SocketSendRequest, SocketType,
};
use source::IcmpRawSocketSource;

const ICMP_PROTOCOL_NUMBER: u16 = 1;
// IPv4 total_len is 16-bit and R0 always forms the minimum 20-byte header.
const ICMP_RAW_MAX_MESSAGE_BYTES: usize = u16::MAX as usize - 20;

static_assert!(NET_ICMP_RAW_DEFAULT_TTL > 0);

#[derive(Clone, Copy)]
struct IcmpRawFamilyState {
    ttl: u8,
    tos: u8,
    /// ABI-only projection captured from a successful normalized connect.
    /// Stack peer address is the association truth; this port never drives
    /// send destination, receive filtering, readiness, or lifecycle.
    peer_port_projection: Option<u16>,
}

impl IcmpRawFamilyState {
    const fn default_from_kconfig() -> Self {
        Self {
            ttl: NET_ICMP_RAW_DEFAULT_TTL,
            tos: NET_ICMP_RAW_DEFAULT_TOS,
            peer_port_projection: None,
        }
    }

    fn egress(self) -> IcmpRawEgressPolicy {
        IcmpRawEgressPolicy::new(self.ttl, self.tos).expect("ICMP raw policy admitted a zero TTL")
    }
}

#[derive(Opaque)]
struct IcmpRawSendSnapshot {
    destination: Ipv4Address,
    policy: IcmpRawEgressPolicy,
    selection: IcmpRawSendSelection,
}

#[derive(Opaque)]
struct IcmpRawSocketFile {
    source: Arc<IcmpRawSocketSource>,
    /// Serializes association operations with the TTL/TOS snapshot used by a
    /// send. Stack remains the only association/filter owner; this guard owns
    /// only family policy and operation ordering.
    operation: Mutex<IcmpRawFamilyState>,
}

impl core::fmt::Debug for IcmpRawSocketFile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("IcmpRawSocketFile").finish_non_exhaustive()
    }
}

impl IcmpRawSocketFile {
    fn new(source: Arc<IcmpRawSocketSource>) -> Self {
        Self {
            source,
            operation: Mutex::new(IcmpRawFamilyState::default_from_kconfig()),
        }
    }

    fn endpoint(&self) -> Option<IcmpRawEndpointPort> {
        self.source.endpoint()
    }
}

/// Owns rollback authority until the common Socket description is published.
/// Capability clones never own semantic lifetime.
#[derive(Opaque)]
struct IcmpRawSocketCreation {
    source: Option<Arc<IcmpRawSocketSource>>,
}

impl IcmpRawSocketCreation {
    fn commit(&mut self) {
        self.source.take();
    }
}

impl Drop for IcmpRawSocketCreation {
    fn drop(&mut self) {
        let Some(source) = self.source.take() else {
            return;
        };
        let result = source.retire();
        assert!(
            result.is_ok(),
            "ICMP raw socket creation rollback lost its Endpoint identity"
        );
    }
}

fn raw_private(private: &AnyOpaque) -> &IcmpRawSocketFile {
    private
        .cast::<IcmpRawSocketFile>()
        .expect("ICMP raw SocketOps used without ICMP raw private state")
}

fn prepare_icmp_raw_socket() -> Result<SocketPreparation, SysError> {
    let endpoint = create_endpoint().map_err(|error| match error {
        anemone_net_api::icmp_raw::IcmpRawCreateError::EndpointCapacity => SysError::NoBufferSpace,
    })?;
    let source = match IcmpRawSocketSource::try_new(endpoint.clone()) {
        Ok(source) => source,
        Err(error) => {
            let retired = endpoint.retire();
            assert!(
                retired.is_ok(),
                "ICMP raw source allocation rollback lost its Endpoint"
            );
            return Err(error);
        },
    };
    Ok(SocketPreparation {
        private: AnyOpaque::new(IcmpRawSocketFile::new(source.clone())),
        creation: SocketCreation {
            commit: commit_icmp_raw_socket,
            authority: AnyOpaque::new(IcmpRawSocketCreation {
                source: Some(source),
            }),
        },
    })
}

fn commit_icmp_raw_socket(creation: &mut AnyOpaque) {
    creation
        .cast_mut::<IcmpRawSocketCreation>()
        .expect("ICMP raw creation commit used without ICMP raw creation authority")
        .commit();
}

fn bind_icmp_raw_socket(
    private: &AnyOpaque,
    address: SocketAddress,
) -> Result<(), SocketBindError> {
    let SocketAddress::Ipv4 { address, .. } = address else {
        return Err(SocketBindError::Unsupported);
    };
    let socket = raw_private(private);
    let _operation = socket.operation.lock();
    socket
        .endpoint()
        .ok_or(SocketBindError::Retired)?
        .bind(address)
        .map_err(map_bind_error)
}

fn connect_icmp_raw_socket(
    private: &AnyOpaque,
    address: SocketAddress,
) -> Result<(), SocketConnectError> {
    let socket = raw_private(private);
    let mut operation = socket.operation.lock();
    let endpoint = socket.endpoint().ok_or(SocketConnectError::Retired)?;
    match address {
        SocketAddress::Unspecified => {
            endpoint.disconnect().map_err(map_disconnect_error)?;
            operation.peer_port_projection = None;
            Ok(())
        },
        SocketAddress::Ipv4 { address, port } => {
            endpoint.connect(address).map_err(map_connect_error)?;
            operation.peer_port_projection = Some(port);
            Ok(())
        },
        SocketAddress::UnixPathname(_) => Err(SocketConnectError::Unsupported),
    }
}

fn query_local_address(
    private: &AnyOpaque,
    sink: &mut dyn SocketAddressSink,
) -> Result<(), SocketQueryError> {
    let socket = raw_private(private);
    let _operation = socket.operation.lock();
    let config = socket
        .endpoint()
        .ok_or(SocketQueryError::Retired)?
        .config()
        .map_err(map_query_error)?;
    let address = config
        .association()
        .local()
        .unwrap_or(Ipv4Address::UNSPECIFIED);
    // Linux exposes the raw protocol in sin_port even before an explicit bind.
    sink.copy_address(Some(SocketAddress::Ipv4 {
        address,
        port: ICMP_PROTOCOL_NUMBER,
    }))
    .map_err(SocketQueryError::Copy)
}

fn query_peer_address(
    private: &AnyOpaque,
    sink: &mut dyn SocketAddressSink,
) -> Result<(), SocketQueryError> {
    let socket = raw_private(private);
    let operation = socket.operation.lock();
    let config = socket
        .endpoint()
        .ok_or(SocketQueryError::Retired)?
        .config()
        .map_err(map_query_error)?;
    let peer = match config.association().peer() {
        Some(peer) => peer,
        None => {
            assert!(
                operation.peer_port_projection.is_none(),
                "ICMP raw peer port projection survived disconnect"
            );
            return Err(SocketQueryError::NotConnected);
        },
    };
    let port = operation
        .peer_port_projection
        .expect("Stack peer association has no raw Socket port projection");
    sink.copy_address(Some(SocketAddress::Ipv4 {
        address: peer,
        port,
    }))
    .map_err(SocketQueryError::Copy)
}

fn raw_is_accepting(_private: &AnyOpaque) -> Result<bool, SocketQueryError> {
    Ok(false)
}

fn send_icmp_raw_socket(
    private: &AnyOpaque,
    request: SocketSendRequest<'_>,
) -> Result<usize, SocketSendError> {
    let SocketSendRequest::Datagram {
        destination,
        payload,
        operation,
    } = request
    else {
        return Err(SocketSendError::Unsupported);
    };
    let explicit_destination = match destination {
        Some(SocketAddress::Ipv4 { address, .. }) => Some(address),
        None => None,
        Some(SocketAddress::Unspecified | SocketAddress::UnixPathname(_)) => {
            return Err(SocketSendError::Unsupported);
        },
    };

    let socket = raw_private(private);
    let snapshot = prepare_icmp_raw_send(socket, explicit_destination, operation)?;
    let payload = payload
        .bytes(ICMP_RAW_MAX_MESSAGE_BYTES)
        .map_err(SocketSendError::Copy)?;
    let len = payload.len();
    socket
        .endpoint()
        .ok_or(SocketSendError::Retired)?
        .send_prepared(
            &snapshot.selection,
            snapshot.destination,
            snapshot.policy,
            payload,
        )
        .map_err(map_send_error)?;
    Ok(len)
}

fn prepare_icmp_raw_send<'a>(
    socket: &IcmpRawSocketFile,
    explicit_destination: Option<Ipv4Address>,
    operation: &'a mut SocketDatagramSendOperation,
) -> Result<&'a IcmpRawSendSnapshot, SocketSendError> {
    if operation.family_snapshot::<IcmpRawSendSnapshot>().is_none() {
        let policy = socket.operation.lock();
        let endpoint = socket.endpoint().ok_or(SocketSendError::Retired)?;
        let destination = match explicit_destination {
            Some(destination) => destination,
            None => endpoint
                .config()
                .map_err(map_send_query_error)?
                .association()
                .peer()
                .ok_or(SocketSendError::DestinationRequired)?,
        };
        let selection = endpoint.prepare_send(destination).map_err(map_send_error)?;
        operation.install_family_snapshot(IcmpRawSendSnapshot {
            destination,
            policy: policy.egress(),
            selection,
        });
    }
    operation
        .family_snapshot::<IcmpRawSendSnapshot>()
        .ok_or_else(|| panic!("ICMP raw send reused another family's datagram operation snapshot"))
}

fn receive_icmp_raw_socket(
    private: &AnyOpaque,
    request: SocketReceiveRequest<'_>,
) -> Result<SocketReceiveOutcome, SocketReceiveError> {
    let SocketReceiveRequest::Datagram { sink, flags } = request else {
        return Err(SocketReceiveError::Unsupported);
    };
    let socket = raw_private(private);
    let _operation = socket.operation.lock();
    let packet = socket
        .endpoint()
        .ok_or(SocketReceiveError::Retired)?
        .receive(flags.peek)
        .map_err(map_receive_error)?;
    copy_received_packet(packet, sink)
}

fn copy_received_packet(
    packet: IcmpRawReceivedPacket,
    sink: &mut dyn super::SocketReceiveSink,
) -> Result<SocketReceiveOutcome, SocketReceiveError> {
    let bytes = packet.bytes();
    assert!(
        bytes.len() >= 20,
        "Stack returned a truncated IPv4 datagram to ICMP raw Socket"
    );
    let peer = Ipv4Address::new(bytes[12..16].try_into().unwrap());
    let packet_length = bytes.len();
    let copied = sink
        .copy_datagram(
            bytes,
            SocketAddress::Ipv4 {
                address: peer,
                port: 0,
            },
        )
        .map_err(SocketReceiveError::Copy)?;
    Ok(SocketReceiveOutcome::datagram(copied, packet_length))
}

fn query_icmp_raw_option(
    private: &AnyOpaque,
    query: SocketOptionQuery,
) -> Result<SocketOptionValue, SocketOptionError> {
    let socket = raw_private(private);
    let policy = socket.operation.lock();
    let endpoint = socket.endpoint().ok_or(SocketOptionError::Retired)?;
    match query {
        SocketOptionQuery::Ipv4TimeToLive => Ok(SocketOptionValue::Ipv4TimeToLive(policy.ttl)),
        SocketOptionQuery::Ipv4TypeOfService => {
            Ok(SocketOptionValue::Ipv4TypeOfService(policy.tos))
        },
        SocketOptionQuery::IcmpTypeFilter => {
            let filter = endpoint.config().map_err(map_option_query_error)?.filter();
            Ok(SocketOptionValue::IcmpTypeFilter(filter.blocked_types()))
        },
    }
}

fn mutate_icmp_raw_option(
    private: &AnyOpaque,
    mutation: SocketOptionMutation,
) -> Result<(), SocketOptionError> {
    let socket = raw_private(private);
    let mut policy = socket.operation.lock();
    let endpoint = socket.endpoint().ok_or(SocketOptionError::Retired)?;
    match mutation {
        SocketOptionMutation::Ipv4TimeToLive(0) => Err(SocketOptionError::InvalidValue),
        SocketOptionMutation::Ipv4TimeToLive(ttl) => {
            policy.ttl = ttl;
            Ok(())
        },
        SocketOptionMutation::Ipv4TypeOfService(tos) => {
            policy.tos = tos;
            Ok(())
        },
        SocketOptionMutation::IcmpTypeFilter(blocked_types) => endpoint
            .set_filter(IcmpRawTypeFilter::from_blocked_types(blocked_types))
            .map_err(map_option_mutation_error),
    }
}

fn poll_icmp_raw_socket(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    raw_private(private).source.poll(request)
}

fn final_release_icmp_raw_socket(private: &AnyOpaque) {
    // Source retirement first withdraws association, reverse publication, and
    // routes. No sleeping operation mutex or fd-table lock participates.
    let result = raw_private(private).source.retire();
    assert!(
        result.is_ok(),
        "ICMP raw final release lost its Endpoint identity"
    );
}

fn map_bind_error(error: BindError) -> SocketBindError {
    match error {
        BindError::AddressUnavailable => SocketBindError::AddressUnavailable,
        BindError::Stack(IcmpRawMutationError::UnknownEndpoint) => SocketBindError::Retired,
        BindError::Stack(IcmpRawMutationError::InvalidAssociation) => SocketBindError::AlreadyBound,
    }
}

fn map_connect_error(error: ConnectError) -> SocketConnectError {
    match error {
        ConnectError::NoRoute | ConnectError::InterfaceUnavailable => {
            SocketConnectError::Operation(SysError::NetworkUnreachable)
        },
        ConnectError::SourceUnavailable => {
            SocketConnectError::Operation(SysError::AddressNotAvailable)
        },
        ConnectError::Stack(IcmpRawMutationError::UnknownEndpoint) => SocketConnectError::Retired,
        ConnectError::Stack(IcmpRawMutationError::InvalidAssociation) => {
            SocketConnectError::InvalidState
        },
    }
}

fn map_disconnect_error(error: IcmpRawMutationError) -> SocketConnectError {
    match error {
        IcmpRawMutationError::UnknownEndpoint => SocketConnectError::Retired,
        IcmpRawMutationError::InvalidAssociation => SocketConnectError::InvalidState,
    }
}

fn map_query_error(error: IcmpRawQueryError) -> SocketQueryError {
    match error {
        IcmpRawQueryError::UnknownEndpoint => SocketQueryError::Retired,
    }
}

fn map_send_query_error(error: IcmpRawQueryError) -> SocketSendError {
    match error {
        IcmpRawQueryError::UnknownEndpoint => SocketSendError::Retired,
    }
}

fn map_send_error(error: SendError) -> SocketSendError {
    match error {
        SendError::NoRoute | SendError::InterfaceUnavailable => SocketSendError::NetworkUnreachable,
        SendError::SourceUnavailable => SocketSendError::AddressUnavailable,
        SendError::Stack(IcmpRawSendError::UnknownEndpoint) => SocketSendError::Retired,
        SendError::Stack(IcmpRawSendError::UnknownInterface) => SocketSendError::NetworkUnreachable,
        SendError::Stack(IcmpRawSendError::UnsupportedSource) => {
            SocketSendError::AddressUnavailable
        },
        SendError::Stack(IcmpRawSendError::InvalidDestination) => {
            SocketSendError::InvalidDestination
        },
        SendError::Stack(IcmpRawSendError::MessageTooLong { .. }) => {
            SocketSendError::MessageTooLong
        },
        SendError::Stack(IcmpRawSendError::WouldBlock) => SocketSendError::WouldBlock,
    }
}

fn map_receive_error(error: IcmpRawReceiveError) -> SocketReceiveError {
    match error {
        IcmpRawReceiveError::UnknownEndpoint => SocketReceiveError::Retired,
        IcmpRawReceiveError::WouldBlock => SocketReceiveError::WouldBlock,
    }
}

fn map_option_query_error(error: IcmpRawQueryError) -> SocketOptionError {
    match error {
        IcmpRawQueryError::UnknownEndpoint => SocketOptionError::Retired,
    }
}

fn map_option_mutation_error(error: IcmpRawMutationError) -> SocketOptionError {
    match error {
        IcmpRawMutationError::UnknownEndpoint => SocketOptionError::Retired,
        IcmpRawMutationError::InvalidAssociation => SocketOptionError::InvalidValue,
    }
}

pub(super) static ICMP_RAW_SOCKET_OPS: SocketOps = SocketOps {
    socket_type: SocketType::Ipv4IcmpRaw,
    payload_io: SocketPayloadIo::Datagram,
    create: Some(prepare_icmp_raw_socket),
    create_pair: None,
    bind: Some(bind_icmp_raw_socket),
    listen: None,
    connect: Some(connect_icmp_raw_socket),
    accept: None,
    shutdown: None,
    local_address: Some(query_local_address),
    peer_address: Some(query_peer_address),
    accepting: raw_is_accepting,
    send: Some(send_icmp_raw_socket),
    receive: Some(receive_icmp_raw_socket),
    query_option: Some(query_icmp_raw_option),
    mutate_option: Some(mutate_icmp_raw_option),
    poll: poll_icmp_raw_socket,
    final_release: final_release_icmp_raw_socket,
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use crate::{
        fs::socket::{
            SocketReceiveSink, SocketSendPayload, prepare_socket, retry_socket_send,
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

    struct PrefixCapture<'a>(&'a mut [u8], Option<SocketAddress>);

    impl SocketReceiveSink for PrefixCapture<'_> {
        fn copy_datagram(
            &mut self,
            payload: &[u8],
            peer: SocketAddress,
        ) -> Result<usize, SysError> {
            let copied = self.0.len().min(payload.len());
            self.0[..copied].copy_from_slice(&payload[..copied]);
            self.1 = Some(peer);
            Ok(copied)
        }
    }

    struct FaultSendPayload;

    impl SocketSendPayload for FaultSendPayload {
        fn bytes(&mut self, _maximum: usize) -> Result<&[u8], SysError> {
            Err(SysError::BadAddress)
        }
    }

    #[kunit]
    fn descriptor_creation_guard_and_final_release_share_one_lifecycle() {
        let (rolled_back_file, creation) =
            prepare_socket(&ICMP_RAW_SOCKET_OPS).expect("KUnit raw endpoint must fit");
        let rolled_back = socket_from_file(&rolled_back_file).unwrap();
        assert_eq!(rolled_back.socket_type(), SocketType::Ipv4IcmpRaw);
        drop(creation);
        assert_eq!(
            rolled_back.copy_local_address(&mut AddressCapture::default()),
            Err(SocketQueryError::Retired)
        );

        let (file, creation) =
            prepare_socket(&ICMP_RAW_SOCKET_OPS).expect("KUnit raw endpoint must fit");
        creation.commit();
        (socket_file_desc_ops().final_release.unwrap())(OpenedFileFinalReleaseCtx {
            file: &file,
            access: OpenAccessMode::ReadWrite,
            notification_suppressed: true,
        });
        assert_eq!(
            socket_from_file(&file)
                .unwrap()
                .copy_local_address(&mut AddressCapture::default()),
            Err(SocketQueryError::Retired)
        );
    }

    #[kunit]
    fn association_queries_options_and_disconnect_use_owner_snapshots() {
        let (file, creation) =
            prepare_socket(&ICMP_RAW_SOCKET_OPS).expect("KUnit raw endpoint must fit");
        let socket = socket_from_file(&file).unwrap();
        assert_eq!(
            socket.query_option(SocketOptionQuery::Ipv4TimeToLive),
            Ok(SocketOptionValue::Ipv4TimeToLive(NET_ICMP_RAW_DEFAULT_TTL))
        );
        assert_eq!(
            socket.mutate_option(SocketOptionMutation::Ipv4TimeToLive(0)),
            Err(SocketOptionError::InvalidValue)
        );
        socket
            .mutate_option(SocketOptionMutation::Ipv4TimeToLive(37))
            .unwrap();
        socket
            .mutate_option(SocketOptionMutation::Ipv4TypeOfService(0xb9))
            .unwrap();
        socket
            .mutate_option(SocketOptionMutation::IcmpTypeFilter(1 << 8))
            .unwrap();
        assert_eq!(
            socket.query_option(SocketOptionQuery::Ipv4TimeToLive),
            Ok(SocketOptionValue::Ipv4TimeToLive(37))
        );
        assert_eq!(
            socket.query_option(SocketOptionQuery::Ipv4TypeOfService),
            Ok(SocketOptionValue::Ipv4TypeOfService(0xb9))
        );
        assert_eq!(
            socket.query_option(SocketOptionQuery::IcmpTypeFilter),
            Ok(SocketOptionValue::IcmpTypeFilter(1 << 8))
        );

        socket
            .bind(SocketAddress::Ipv4 {
                address: Ipv4Address::UNSPECIFIED,
                port: 0,
            })
            .unwrap();
        let mut local = AddressCapture::default();
        socket.copy_local_address(&mut local).unwrap();
        assert_eq!(
            local.0,
            Some(SocketAddress::Ipv4 {
                address: Ipv4Address::UNSPECIFIED,
                port: ICMP_PROTOCOL_NUMBER,
            })
        );
        assert!(
            socket
                .connect(SocketAddress::Ipv4 {
                    address: Ipv4Address::LOOPBACK,
                    port: 7,
                })
                .is_ok()
        );
        let mut peer = AddressCapture::default();
        socket.copy_peer_address(&mut peer).unwrap();
        assert_eq!(
            peer.0,
            Some(SocketAddress::Ipv4 {
                address: Ipv4Address::LOOPBACK,
                port: 7,
            })
        );
        assert!(socket.connect(SocketAddress::Unspecified).is_ok());
        assert_eq!(
            socket.copy_peer_address(&mut AddressCapture::default()),
            Err(SocketQueryError::NotConnected)
        );
        drop(creation);
    }

    #[kunit]
    fn common_payload_io_dispatches_raw_as_a_datagram_family() {
        let (file, creation) =
            prepare_socket(&ICMP_RAW_SOCKET_OPS).expect("KUnit raw endpoint must fit");
        let socket = socket_from_file(&file).unwrap();
        assert_eq!(socket.payload_io(), SocketPayloadIo::Datagram);

        let mut byte = [0u8; 1];
        assert_eq!(
            file.read_with_ctx(&mut byte, FileIoCtx::new(FileOpStatusFlags::NONBLOCK),),
            Err(SysError::Again)
        );
        assert_eq!(file.write(b"x"), Err(SysError::DestinationAddressRequired));
        drop(creation);
    }

    #[kunit]
    fn blocking_retry_keeps_one_destination_policy_and_selection_snapshot() {
        let (file, creation) =
            prepare_socket(&ICMP_RAW_SOCKET_OPS).expect("KUnit raw endpoint must fit");
        let socket = socket_from_file(&file).unwrap();
        let first_peer = Ipv4Address::LOOPBACK;
        let second_peer = Ipv4Address::new([127, 0, 0, 2]);
        socket
            .mutate_option(SocketOptionMutation::Ipv4TimeToLive(37))
            .unwrap();
        socket
            .mutate_option(SocketOptionMutation::Ipv4TypeOfService(0x24))
            .unwrap();
        assert!(
            socket
                .connect(SocketAddress::Ipv4 {
                    address: first_peer,
                    port: 7,
                })
                .is_ok()
        );

        let task = get_current_task();
        let mut operation = SocketDatagramSendOperation::new();
        let mut payload = FaultSendPayload;
        let mut attempts = 0;
        assert_eq!(
            retry_socket_send(
                "ICMP raw KUnit snapshot retry",
                &task,
                &file,
                false,
                false,
                || {
                    attempts += 1;
                    assert_eq!(
                        socket.send(SocketSendRequest::Datagram {
                            destination: None,
                            payload: &mut payload,
                            operation: &mut operation,
                        }),
                        Err(SocketSendError::Copy(SysError::BadAddress))
                    );
                    let snapshot = operation
                        .family_snapshot::<IcmpRawSendSnapshot>()
                        .expect("raw send attempt must install its immutable snapshot");
                    assert_eq!(snapshot.destination, first_peer);
                    assert_eq!(snapshot.policy.ttl(), 37);
                    assert_eq!(snapshot.policy.tos(), 0x24);
                    if attempts == 1 {
                        socket
                            .mutate_option(SocketOptionMutation::Ipv4TimeToLive(88))
                            .unwrap();
                        socket
                            .mutate_option(SocketOptionMutation::Ipv4TypeOfService(0x48))
                            .unwrap();
                        assert!(
                            socket
                                .connect(SocketAddress::Ipv4 {
                                    address: second_peer,
                                    port: 9,
                                })
                                .is_ok()
                        );
                        Err(SocketSendError::WouldBlock)
                    } else {
                        Ok(0)
                    }
                },
                |_| SysError::IO,
            ),
            Ok(0)
        );
        assert_eq!(attempts, 2);
        drop(creation);
    }

    #[kunit]
    fn detached_packet_projects_peer_prefix_and_complete_length() {
        let mut packet = vec![0u8; 24];
        packet[0] = 0x45;
        packet[12..16].copy_from_slice(&[203, 0, 113, 7]);
        let mut prefix = [0u8; 8];
        let mut sink = PrefixCapture(&mut prefix, None);
        assert_eq!(
            copy_received_packet(
                IcmpRawReceivedPacket::from_owner_detach(packet.clone()),
                &mut sink,
            ),
            Ok(SocketReceiveOutcome::datagram(8, 24))
        );
        let peer = sink.1.clone();
        drop(sink);
        assert_eq!(prefix, packet[..8]);
        assert_eq!(
            peer,
            Some(SocketAddress::Ipv4 {
                address: Ipv4Address::new([203, 0, 113, 7]),
                port: 0,
            })
        );
    }
}
