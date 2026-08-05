//! Family-neutral Socket front, static dispatch, and opened-description hooks.

mod file;
mod operation;

use anemone_net_api::Ipv4Address;

use crate::{
    prelude::*,
    utils::any_opaque::{AnyOpaque, Opaque},
};

#[cfg(feature = "kunit")]
use file::{SliceReadSink, SliceWriteSource};
use file::{prepare_socket_file, prepare_socket_file_at, prepare_socket_path};
pub(super) use file::{socket_file_desc_ops, socket_from_file};
pub(super) use operation::{retry_socket_receive, retry_socket_send, wait_for_socket_operation};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketType {
    Ipv4Udp,
    Ipv4IcmpRaw,
    Ipv4Tcp,
    UnixStream,
    UnixSeqpacket,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SocketAddress {
    Unspecified,
    Ipv4 { address: Ipv4Address, port: u16 },
    UnixPathname(Arc<str>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketBindError {
    Unsupported,
    Retired,
    AlreadyBound,
    AddressInUse,
    AddressUnavailable,
    ResourceExhausted,
    Operation(SysError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketQueryError {
    Unsupported,
    Retired,
    NotConnected,
    Copy(SysError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketListenError {
    Unsupported,
    Retired,
    InvalidState,
    ResourceExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketShutdown {
    Read,
    Write,
    ReadWrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketShutdownError {
    Unsupported,
    Retired,
    NotConnected,
}

pub(super) enum SocketConnectError {
    Unsupported,
    Retired,
    InvalidState,
    Started,
    InProgress,
    AlreadyConnected,
    ConnectionRefused,
    ConnectionTimedOut,
    ProtocolTypeMismatch,
    WouldBlock(SocketWait),
    Operation(SysError),
}

pub(super) enum SocketAcceptError {
    Unsupported,
    Retired,
    InvalidState,
    ResourceExhausted,
    WouldBlock(SocketWait),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketSendError {
    Unsupported,
    Retired,
    NotConnected,
    AlreadyConnected,
    InvalidState,
    AddressInUse,
    AddressUnavailable,
    ResourceExhausted,
    NetworkUnreachable,
    DestinationRequired,
    InvalidDestination,
    MessageTooLong,
    PeerClosed,
    WouldBlock,
    Copy(SysError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketReceiveError {
    Unsupported,
    Retired,
    InvalidState,
    WouldBlock,
    Copy(SysError),
}

pub(super) trait SocketSendPayload {
    fn bytes(&mut self, maximum: usize) -> Result<&[u8], SysError>;
}

/// Opaque family snapshot retained for one datagram send operation.
///
/// A blocking retry must reuse operation-local destination, policy, and route
/// selection without retaining a family lock or exposing those values to the
/// common front. The family installs at most one immutable snapshot here; the
/// object is discarded when the syscall or FileOps operation returns.
pub(super) struct SocketDatagramSendOperation {
    family_snapshot: Option<AnyOpaque>,
}

impl SocketDatagramSendOperation {
    pub(super) const fn new() -> Self {
        Self {
            family_snapshot: None,
        }
    }

    pub(super) fn family_snapshot<T: Opaque>(&self) -> Option<&T> {
        self.family_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.cast::<T>())
    }

    pub(super) fn install_family_snapshot<T: Opaque>(&mut self, snapshot: T) -> &T {
        assert!(
            self.family_snapshot.is_none(),
            "Socket datagram send operation installed two family snapshots"
        );
        self.family_snapshot = Some(AnyOpaque::new(snapshot));
        self.family_snapshot::<T>()
            .expect("fresh Socket datagram send snapshot changed type")
    }
}

pub(super) trait SocketAddressSink {
    fn copy_address(&mut self, address: Option<SocketAddress>) -> Result<(), SysError>;
}

pub(super) trait SocketReceiveSink {
    fn copy_datagram(&mut self, payload: &[u8], peer: SocketAddress) -> Result<usize, SysError>;
}

pub(super) trait SocketReadSink {
    fn remaining(&self) -> usize;

    fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError>;

    /// Seqpacket copyout must distinguish a short destination from a fault
    /// after a selected prefix. Stream I/O keeps partial-progress semantics.
    fn copy_exact(&mut self, bytes: &[u8]) -> Result<(), SysError> {
        let copied = self.copy_bytes(bytes)?;
        if copied == bytes.len() {
            Ok(())
        } else {
            Err(SysError::BadAddress)
        }
    }
}

pub(super) trait SocketWriteSource {
    fn remaining(&self) -> usize;

    fn copy_bytes(&mut self, bytes: &mut [u8]) -> Result<usize, SysError>;

    /// Stage one complete seqpacket payload before the owner-local commit.
    fn copy_exact(&mut self, bytes: &mut [u8]) -> Result<(), SysError> {
        let copied = self.copy_bytes(bytes)?;
        if copied == bytes.len() {
            Ok(())
        } else {
            Err(SysError::BadAddress)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketStreamDestination {
    Absent,
    Present,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SocketReceiveFlags {
    pub(super) peek: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketReceiveOutcome {
    ByteStream { copied: usize },
    Datagram { copied: usize, packet_length: usize },
    Seqpacket { copied: usize, record_length: usize },
}

impl SocketReceiveOutcome {
    pub(super) const fn byte_stream(copied: usize) -> Self {
        Self::ByteStream { copied }
    }

    pub(super) const fn datagram(copied: usize, packet_length: usize) -> Self {
        Self::Datagram {
            copied,
            packet_length,
        }
    }

    pub(super) const fn seqpacket(copied: usize, record_length: usize) -> Self {
        Self::Seqpacket {
            copied,
            record_length,
        }
    }

    pub(super) const fn copied(self) -> usize {
        match self {
            Self::ByteStream { copied }
            | Self::Datagram { copied, .. }
            | Self::Seqpacket { copied, .. } => copied,
        }
    }

    pub(super) const fn packet_length(self) -> Option<usize> {
        match self {
            Self::ByteStream { .. } => None,
            Self::Datagram { packet_length, .. } => Some(packet_length),
            Self::Seqpacket { record_length, .. } => Some(record_length),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketOptionQuery {
    Ipv4TimeToLive,
    Ipv4TypeOfService,
    IcmpTypeFilter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketOptionValue {
    Ipv4TimeToLive(u8),
    Ipv4TypeOfService(u8),
    IcmpTypeFilter(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketOptionMutation {
    Ipv4TimeToLive(u8),
    Ipv4TypeOfService(u8),
    IcmpTypeFilter(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketOptionError {
    Unsupported,
    Retired,
    InvalidValue,
}

pub(super) enum SocketSendRequest<'a> {
    Datagram {
        destination: Option<SocketAddress>,
        payload: &'a mut dyn SocketSendPayload,
        operation: &'a mut SocketDatagramSendOperation,
    },
    Stream {
        source: &'a mut dyn SocketWriteSource,
        destination: SocketStreamDestination,
    },
    Seqpacket {
        source: &'a mut dyn SocketWriteSource,
        destination: SocketStreamDestination,
    },
}

pub(super) enum SocketReceiveRequest<'a> {
    Datagram {
        sink: &'a mut dyn SocketReceiveSink,
        flags: SocketReceiveFlags,
    },
    Stream {
        sink: &'a mut dyn SocketReadSink,
        flags: SocketReceiveFlags,
    },
    Seqpacket {
        sink: &'a mut dyn SocketReadSink,
        flags: SocketReceiveFlags,
    },
}

pub(super) struct SocketPreparation {
    pub(super) private: AnyOpaque,
    pub(super) creation: SocketCreation,
}

pub(super) struct SocketPairPreparation {
    pub(super) first_private: AnyOpaque,
    pub(super) second_private: AnyOpaque,
}

pub(super) struct SocketWait {
    private: AnyOpaque,
    poll: for<'a> fn(&AnyOpaque, &PollRequest<'a>) -> Result<PollRegisterResult, SysError>,
}

impl SocketWait {
    pub(super) fn new(
        private: AnyOpaque,
        poll: for<'a> fn(&AnyOpaque, &PollRequest<'a>) -> Result<PollRegisterResult, SysError>,
    ) -> Self {
        Self { private, poll }
    }

    pub(super) fn poll(&self, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
        (self.poll)(&self.private, request)
    }
}

pub(super) struct SocketAcceptItem {
    pub(super) private: AnyOpaque,
    pub(super) peer_address: Option<SocketAddress>,
}

pub(super) type SocketSendOp =
    for<'a> fn(&AnyOpaque, SocketSendRequest<'a>) -> Result<usize, SocketSendError>;
pub(super) type SocketSendWaitOp = fn(&AnyOpaque, usize) -> SocketWait;
pub(super) type SocketReceiveOp = for<'a> fn(
    &AnyOpaque,
    SocketReceiveRequest<'a>,
) -> Result<SocketReceiveOutcome, SocketReceiveError>;

/// Complete type/data-plane capability bundle for one static Socket descriptor.
///
/// Every variant carries the handlers required by that operation shape, and
/// seqpacket additionally requires its payload-specific send predicate.
#[derive(Clone, Copy)]
pub(super) enum SocketIoOps {
    ByteStream {
        socket_type: SocketType,
        send: SocketSendOp,
        receive: SocketReceiveOp,
    },
    Datagram {
        socket_type: SocketType,
        send: SocketSendOp,
        receive: SocketReceiveOp,
    },
    Seqpacket {
        socket_type: SocketType,
        send: SocketSendOp,
        send_wait: SocketSendWaitOp,
        receive: SocketReceiveOp,
    },
}

impl SocketIoOps {
    const fn socket_type(self) -> SocketType {
        match self {
            Self::ByteStream { socket_type, .. }
            | Self::Datagram { socket_type, .. }
            | Self::Seqpacket { socket_type, .. } => socket_type,
        }
    }

    const fn send(self) -> SocketSendOp {
        match self {
            Self::ByteStream { send, .. }
            | Self::Datagram { send, .. }
            | Self::Seqpacket { send, .. } => send,
        }
    }

    const fn receive(self) -> SocketReceiveOp {
        match self {
            Self::ByteStream { receive, .. }
            | Self::Datagram { receive, .. }
            | Self::Seqpacket { receive, .. } => receive,
        }
    }
}

pub(super) struct SocketOps {
    pub(super) io: SocketIoOps,
    pub(super) create: Option<fn() -> Result<SocketPreparation, SysError>>,
    pub(super) create_pair: Option<fn() -> Result<SocketPairPreparation, SysError>>,
    pub(super) bind: Option<fn(&AnyOpaque, SocketAddress) -> Result<(), SocketBindError>>,
    pub(super) listen: Option<fn(&AnyOpaque, i32) -> Result<(), SocketListenError>>,
    pub(super) connect: Option<fn(&AnyOpaque, SocketAddress) -> Result<(), SocketConnectError>>,
    pub(super) accept: Option<fn(&AnyOpaque) -> Result<SocketAcceptItem, SocketAcceptError>>,
    pub(super) shutdown: Option<fn(&AnyOpaque, SocketShutdown) -> Result<(), SocketShutdownError>>,
    pub(super) local_address:
        Option<fn(&AnyOpaque, &mut dyn SocketAddressSink) -> Result<(), SocketQueryError>>,
    pub(super) peer_address:
        Option<fn(&AnyOpaque, &mut dyn SocketAddressSink) -> Result<(), SocketQueryError>>,
    pub(super) accepting: fn(&AnyOpaque) -> Result<bool, SocketQueryError>,
    pub(super) query_option:
        Option<fn(&AnyOpaque, SocketOptionQuery) -> Result<SocketOptionValue, SocketOptionError>>,
    pub(super) mutate_option:
        Option<fn(&AnyOpaque, SocketOptionMutation) -> Result<(), SocketOptionError>>,
    pub(super) poll:
        for<'a> fn(&AnyOpaque, &PollRequest<'a>) -> Result<PollRegisterResult, SysError>,
    pub(super) final_release: fn(&AnyOpaque),
}

impl SocketOps {
    /// The descriptor remains the sole semantic type witness. ABI publication
    /// profiles query it through this narrow surface instead of duplicating a
    /// family tag beside the data-plane capability bundle.
    pub(super) const fn socket_type(&self) -> SocketType {
        self.io.socket_type()
    }
}

#[derive(Opaque)]
pub(super) struct Socket {
    /// This immutable descriptor is the sole type witness. The front does not
    /// cache a family tag or any family-owned readiness/lifecycle fact.
    ops: &'static SocketOps,
    private: AnyOpaque,
}

impl core::fmt::Debug for Socket {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Socket")
            .field("socket_type", &self.socket_type())
            .finish_non_exhaustive()
    }
}

impl Socket {
    pub(super) const fn socket_type(&self) -> SocketType {
        self.ops.socket_type()
    }

    pub(super) const fn io(&self) -> SocketIoOps {
        self.ops.io
    }

    pub(super) fn bind(&self, address: SocketAddress) -> Result<(), SocketBindError> {
        self.ops.bind.ok_or(SocketBindError::Unsupported)?(&self.private, address)
    }

    pub(super) fn listen(&self, backlog: i32) -> Result<(), SocketListenError> {
        self.ops.listen.ok_or(SocketListenError::Unsupported)?(&self.private, backlog)
    }

    pub(super) fn connect(&self, address: SocketAddress) -> Result<(), SocketConnectError> {
        self.ops.connect.ok_or(SocketConnectError::Unsupported)?(&self.private, address)
    }

    pub(super) fn accept(&self) -> Result<AcceptedSocket, SocketAcceptError> {
        let item = self.ops.accept.ok_or(SocketAcceptError::Unsupported)?(&self.private)?;
        Ok(AcceptedSocket {
            ops: self.ops,
            private: Some(item.private),
            peer_address: item.peer_address,
        })
    }

    pub(super) fn shutdown(&self, how: SocketShutdown) -> Result<(), SocketShutdownError> {
        self.ops.shutdown.ok_or(SocketShutdownError::Unsupported)?(&self.private, how)
    }

    pub(super) fn is_accepting(&self) -> Result<bool, SocketQueryError> {
        (self.ops.accepting)(&self.private)
    }

    pub(super) fn copy_local_address(
        &self,
        sink: &mut dyn SocketAddressSink,
    ) -> Result<(), SocketQueryError> {
        self.ops
            .local_address
            .ok_or(SocketQueryError::Unsupported)?(&self.private, sink)
    }

    pub(super) fn copy_peer_address(
        &self,
        sink: &mut dyn SocketAddressSink,
    ) -> Result<(), SocketQueryError> {
        self.ops.peer_address.ok_or(SocketQueryError::Unsupported)?(&self.private, sink)
    }

    pub(super) fn send(&self, request: SocketSendRequest<'_>) -> Result<usize, SocketSendError> {
        (self.ops.io.send())(&self.private, request)
    }

    pub(super) fn seqpacket_send_wait(&self, payload_len: usize) -> SocketWait {
        let SocketIoOps::Seqpacket { send_wait, .. } = self.ops.io else {
            panic!("seqpacket send wait requested from a non-seqpacket I/O bundle")
        };
        send_wait(&self.private, payload_len)
    }

    pub(super) fn receive(
        &self,
        request: SocketReceiveRequest<'_>,
    ) -> Result<SocketReceiveOutcome, SocketReceiveError> {
        (self.ops.io.receive())(&self.private, request)
    }

    pub(super) fn query_option(
        &self,
        query: SocketOptionQuery,
    ) -> Result<SocketOptionValue, SocketOptionError> {
        self.ops
            .query_option
            .ok_or(SocketOptionError::Unsupported)?(&self.private, query)
    }

    pub(super) fn mutate_option(
        &self,
        mutation: SocketOptionMutation,
    ) -> Result<(), SocketOptionError> {
        self.ops
            .mutate_option
            .ok_or(SocketOptionError::Unsupported)?(&self.private, mutation)
    }

    fn poll(&self, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
        (self.ops.poll)(&self.private, request)
    }

    fn final_release(&self) {
        (self.ops.final_release)(&self.private);
    }
}

/// Owns a consumed accept item until its peer address is copied and its file
/// description is fully prepared. Dropping it closes the child instead of
/// requeueing or leaking a connection that the caller never received.
pub(super) struct AcceptedSocket {
    ops: &'static SocketOps,
    private: Option<AnyOpaque>,
    peer_address: Option<SocketAddress>,
}

impl AcceptedSocket {
    pub(super) fn peer_address(&self) -> Option<SocketAddress> {
        self.peer_address.clone()
    }

    pub(super) fn prepare_file(mut self) -> Result<File, SysError> {
        // Allocate every normally fallible file resource before transferring
        // the child private state. A failure here leaves `self.private` owned
        // by this guard, whose Drop closes the consumed child.
        let path = prepare_socket_path()?;
        let private = self
            .private
            .take()
            .expect("accepted Socket private state was consumed twice");
        Ok(prepare_socket_file_at(&path, self.ops, private))
    }
}

impl Drop for AcceptedSocket {
    fn drop(&mut self) {
        if let Some(private) = self.private.as_ref() {
            (self.ops.final_release)(private);
        }
    }
}

/// Owns family rollback authority until the prepared opened description is
/// published. The associated create operations alone interpret that authority.
pub(super) struct SocketCreation {
    pub(super) commit: fn(&mut AnyOpaque),
    pub(super) authority: AnyOpaque,
}

impl SocketCreation {
    pub(super) fn commit(mut self) {
        (self.commit)(&mut self.authority);
    }
}

pub(super) fn prepare_socket(ops: &'static SocketOps) -> Result<(File, SocketCreation), SysError> {
    let create = ops.create.ok_or(SysError::NotSupported)?;
    let SocketPreparation { private, creation } = create()?;
    let file = prepare_socket_file(ops, private)?;
    Ok((file, creation))
}

pub(super) fn prepare_socket_pair(ops: &'static SocketOps) -> Result<(File, File), SysError> {
    let create_pair = ops.create_pair.ok_or(SysError::NotSupported)?;
    let SocketPairPreparation {
        first_private,
        second_private,
    } = create_pair()?;
    let first = prepare_socket_file(ops, first_private)?;
    let second = prepare_socket_file(ops, second_private)?;
    Ok((first, second))
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use crate::fs::socket::{UDP_SOCKET_OPS, UNIX_STREAM_SOCKET_OPS};

    #[kunit]
    fn static_descriptor_is_the_only_socket_type_witness() {
        let (file, creation) = prepare_socket(&UDP_SOCKET_OPS).expect("KUnit UDP socket must fit");
        let socket = socket_from_file(&file).expect("prepared file must be a common Socket");
        assert_eq!(socket.socket_type(), SocketType::Ipv4Udp);
        drop(creation);
    }

    #[kunit]
    fn zero_length_stream_requests_preserve_family_semantics() {
        let (first, second) =
            prepare_socket_pair(&UNIX_STREAM_SOCKET_OPS).expect("Unix KUnit pair must fit");
        let first_socket = socket_from_file(&first).expect("first file must be a Socket");
        let second_socket = socket_from_file(&second).expect("second file must be a Socket");

        let mut byte = SliceWriteSource { bytes: b"x" };
        assert_eq!(
            first_socket.send(SocketSendRequest::Stream {
                source: &mut byte,
                destination: SocketStreamDestination::Absent,
            }),
            Ok(1)
        );
        let mut empty_bytes = [];
        let mut empty = SliceReadSink {
            bytes: &mut empty_bytes,
        };
        assert_eq!(
            second_socket.receive(SocketReceiveRequest::Stream {
                sink: &mut empty,
                flags: SocketReceiveFlags { peek: false },
            }),
            Ok(SocketReceiveOutcome::byte_stream(0))
        );
        let mut received = [0u8; 1];
        let mut sink = SliceReadSink {
            bytes: &mut received,
        };
        assert_eq!(
            second_socket.receive(SocketReceiveRequest::Stream {
                sink: &mut sink,
                flags: SocketReceiveFlags { peek: false },
            }),
            Ok(SocketReceiveOutcome::byte_stream(1))
        );
        assert_eq!(received, *b"x");

        second_socket.final_release();
        let mut empty = SliceWriteSource { bytes: b"" };
        assert_eq!(
            first_socket.send(SocketSendRequest::Stream {
                source: &mut empty,
                destination: SocketStreamDestination::Absent,
            }),
            Err(SocketSendError::PeerClosed)
        );
        let mut byte = SliceWriteSource { bytes: b"x" };
        assert_eq!(
            first_socket.send(SocketSendRequest::Stream {
                source: &mut byte,
                destination: SocketStreamDestination::Absent,
            }),
            Err(SocketSendError::PeerClosed)
        );

        let (udp_file, creation) =
            prepare_socket(&UDP_SOCKET_OPS).expect("KUnit UDP endpoint must fit");
        let udp_socket = socket_from_file(&udp_file).expect("UDP file must be a Socket");
        let mut empty = SliceWriteSource { bytes: b"" };
        assert_eq!(
            udp_socket.send(SocketSendRequest::Stream {
                source: &mut empty,
                destination: SocketStreamDestination::Absent,
            }),
            Err(SocketSendError::Unsupported)
        );
        drop(creation);
    }
}
