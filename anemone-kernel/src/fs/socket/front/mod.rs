//! Family-neutral Socket front, static dispatch, and opened-description hooks.

mod file;
mod operation;

use anemone_net_api::Ipv4Address;

use crate::{prelude::*, utils::any_opaque::AnyOpaque};

#[cfg(feature = "kunit")]
use file::{SliceReadSink, SliceWriteSource};
use file::{prepare_socket_file, prepare_socket_file_at, prepare_socket_path};
pub(super) use file::{socket_file_desc_ops, socket_from_file};
pub(super) use operation::{retry_socket_receive, retry_socket_send, wait_for_socket_operation};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketType {
    Ipv4Udp,
    UnixStream,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SocketAddress {
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
    AlreadyConnected,
    ConnectionRefused,
    WouldBlock(SocketWait),
    Operation(SysError),
}

pub(super) enum SocketAcceptError {
    Unsupported,
    Retired,
    InvalidState,
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
    fn bytes(&mut self) -> Result<&[u8], SysError>;
}

pub(super) trait SocketAddressSink {
    fn copy_address(&mut self, address: Option<SocketAddress>) -> Result<(), SysError>;
}

pub(super) trait SocketReceiveSink {
    fn copy_datagram(&mut self, payload: &[u8], peer: SocketAddress) -> Result<usize, SysError>;
}

pub(super) trait SocketStreamReadSink {
    fn remaining(&self) -> usize;

    fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError>;
}

pub(super) trait SocketStreamWriteSource {
    fn remaining(&self) -> usize;

    fn copy_bytes(&mut self, bytes: &mut [u8]) -> Result<usize, SysError>;
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

pub(super) enum SocketSendRequest<'a> {
    Datagram {
        peer: SocketAddress,
        payload: &'a mut dyn SocketSendPayload,
    },
    Stream {
        source: &'a mut dyn SocketStreamWriteSource,
        destination: SocketStreamDestination,
    },
}

pub(super) enum SocketReceiveRequest<'a> {
    Datagram(&'a mut dyn SocketReceiveSink),
    Stream {
        sink: &'a mut dyn SocketStreamReadSink,
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

pub(super) struct SocketOps {
    pub(super) socket_type: SocketType,
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
    pub(super) send:
        Option<for<'a> fn(&AnyOpaque, SocketSendRequest<'a>) -> Result<usize, SocketSendError>>,
    pub(super) receive: Option<
        for<'a> fn(&AnyOpaque, SocketReceiveRequest<'a>) -> Result<usize, SocketReceiveError>,
    >,
    pub(super) poll:
        for<'a> fn(&AnyOpaque, &PollRequest<'a>) -> Result<PollRegisterResult, SysError>,
    pub(super) final_release: fn(&AnyOpaque),
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
            .field("socket_type", &self.ops.socket_type)
            .finish_non_exhaustive()
    }
}

impl Socket {
    pub(super) const fn socket_type(&self) -> SocketType {
        self.ops.socket_type
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
        self.ops.send.ok_or(SocketSendError::Unsupported)?(&self.private, request)
    }

    pub(super) fn receive(
        &self,
        request: SocketReceiveRequest<'_>,
    ) -> Result<usize, SocketReceiveError> {
        self.ops.receive.ok_or(SocketReceiveError::Unsupported)?(&self.private, request)
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
            Ok(0)
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
            Ok(1)
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
