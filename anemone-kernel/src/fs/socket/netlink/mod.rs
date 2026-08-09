//! AF_NETLINK transport for the accepted read-only diagnostic protocols.

mod codec;

use alloc::{collections::VecDeque, sync::Arc};

use crate::{
    fs::socket::source::SocketPollSource,
    kconfig_defs::{
        NETLINK_PENDING_REPLY_MAX_BYTES, NETLINK_PORT_CAPACITY, NETLINK_REPLY_DATAGRAM_MAX_BYTES,
        NETLINK_REQUEST_MAX_BYTES,
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::{
    SocketAddress, SocketAddressSink, SocketBindError, SocketCreation, SocketIoOps, SocketOps,
    SocketOptionError, SocketOptionMutation, SocketPreparation, SocketQueryError,
    SocketReceiveError, SocketReceiveOutcome, SocketReceiveRequest, SocketReleaseReason,
    SocketSendError, SocketSendRequest, SocketType,
};

const DEFAULT_SEND_BUDGET: usize = 32 * 1024;
const DEFAULT_RECEIVE_BUDGET: usize = 1024 * 1024;
const MINIMUM_BUDGET: usize = 4 * 1024;

const _: () = assert!(NETLINK_PORT_CAPACITY > 0);
const _: () = assert!(NETLINK_REQUEST_MAX_BYTES >= MINIMUM_BUDGET);
const _: () = assert!(NETLINK_REPLY_DATAGRAM_MAX_BYTES >= codec::MINIMUM_ERROR_REPLY_BYTES);
const _: () = assert!(NETLINK_PENDING_REPLY_MAX_BYTES >= NETLINK_REPLY_DATAGRAM_MAX_BYTES);
const _: () = assert!(NETLINK_PENDING_REPLY_MAX_BYTES >= MINIMUM_BUDGET);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NetlinkProtocol {
    Route,
    SockDiag,
}

struct PortRegistry {
    live: Vec<u32>,
    next: u32,
}

impl PortRegistry {
    fn new() -> Self {
        Self {
            live: Vec::with_capacity(NETLINK_PORT_CAPACITY),
            next: 1,
        }
    }

    fn allocate(&mut self) -> Result<u32, SocketBindError> {
        if self.live.len() == NETLINK_PORT_CAPACITY {
            return Err(SocketBindError::ResourceExhausted);
        }
        let port = self.next;
        self.next = self
            .next
            .checked_add(1)
            .expect("netlink boot-local port identity space exhausted");
        self.live.push(port);
        Ok(port)
    }

    fn release(&mut self, port: u32) {
        let index = self
            .live
            .iter()
            .position(|candidate| *candidate == port)
            .expect("netlink final release lost its protocol-scoped port");
        self.live.swap_remove(index);
    }
}

static ROUTE_PORTS: Lazy<SpinLock<PortRegistry>> = Lazy::new(|| SpinLock::new(PortRegistry::new()));
static DIAG_PORTS: Lazy<SpinLock<PortRegistry>> = Lazy::new(|| SpinLock::new(PortRegistry::new()));

fn registry(protocol: NetlinkProtocol) -> &'static SpinLock<PortRegistry> {
    match protocol {
        NetlinkProtocol::Route => &ROUTE_PORTS,
        NetlinkProtocol::SockDiag => &DIAG_PORTS,
    }
}

struct PendingReplies {
    datagrams: VecDeque<Arc<[u8]>>,
    bytes: usize,
}

impl PendingReplies {
    fn empty() -> Self {
        Self {
            datagrams: VecDeque::new(),
            bytes: 0,
        }
    }

    fn required_growth(&self, added: usize) -> Option<usize> {
        let required = self
            .datagrams
            .len()
            .checked_add(added)
            .expect("admitted netlink reply count overflowed");
        (required > self.datagrams.capacity()).then(|| {
            self.datagrams
                .capacity()
                .saturating_mul(2)
                .max(required)
                .max(8)
        })
    }
}

struct TransportFacts {
    port: Option<u32>,
    send_budget: usize,
    receive_budget: usize,
    pending: Option<PendingReplies>,
    retired: bool,
}

struct NetlinkTransport {
    facts: SpinLock<TransportFacts>,
}

impl NetlinkTransport {
    fn new() -> Self {
        Self {
            facts: SpinLock::new(TransportFacts {
                port: None,
                send_budget: DEFAULT_SEND_BUDGET.min(NETLINK_REQUEST_MAX_BYTES),
                receive_budget: DEFAULT_RECEIVE_BUDGET.min(NETLINK_PENDING_REPLY_MAX_BYTES),
                pending: Some(PendingReplies::empty()),
                retired: false,
            }),
        }
    }
}

#[derive(Opaque)]
struct NetlinkSocketFile {
    protocol: NetlinkProtocol,
    transport: Arc<NetlinkTransport>,
    source: Arc<SocketPollSource<Arc<NetlinkTransport>>>,
    /// Serializes bind, reply publication and budget mutation. Network-owner
    /// observation and Linux serialization stay outside this mutex. Final
    /// release never waits here; retirement belongs to the source handoff.
    operation: Mutex<()>,
}

#[derive(Opaque)]
struct NetlinkSocketCreation {
    protocol: NetlinkProtocol,
    source: Option<Arc<SocketPollSource<Arc<NetlinkTransport>>>>,
}

impl NetlinkSocketCreation {
    fn commit(&mut self) {
        self.source.take();
    }
}

impl Drop for NetlinkSocketCreation {
    fn drop(&mut self) {
        if let Some(source) = self.source.take() {
            retire_transport(self.protocol, &source);
        }
    }
}

fn netlink_private(private: &AnyOpaque) -> &NetlinkSocketFile {
    private
        .cast::<NetlinkSocketFile>()
        .expect("netlink SocketOps used without netlink private state")
}

fn prepare(protocol: NetlinkProtocol) -> Result<SocketPreparation, SysError> {
    let transport = Arc::new(NetlinkTransport::new());
    let source = Arc::new(SocketPollSource::try_new()?);
    source.publish(transport.clone());
    Ok(SocketPreparation {
        private: AnyOpaque::new(NetlinkSocketFile {
            protocol,
            transport,
            source: source.clone(),
            operation: Mutex::new(()),
        }),
        creation: SocketCreation {
            commit: commit_creation,
            authority: AnyOpaque::new(NetlinkSocketCreation {
                protocol,
                source: Some(source),
            }),
        },
    })
}

fn prepare_route() -> Result<SocketPreparation, SysError> {
    prepare(NetlinkProtocol::Route)
}

fn prepare_sock_diag() -> Result<SocketPreparation, SysError> {
    prepare(NetlinkProtocol::SockDiag)
}

fn commit_creation(authority: &mut AnyOpaque) {
    authority
        .cast_mut::<NetlinkSocketCreation>()
        .expect("netlink creation commit lost its authority")
        .commit();
}

fn ensure_bound(file: &NetlinkSocketFile) -> Result<u32, SocketBindError> {
    {
        let facts = file.transport.facts.lock();
        if facts.retired {
            return Err(SocketBindError::Retired);
        }
        if let Some(port) = facts.port {
            return Ok(port);
        }
    }
    let port = registry(file.protocol).lock().allocate()?;
    let mut facts = file.transport.facts.lock();
    if facts.retired {
        drop(facts);
        registry(file.protocol).lock().release(port);
        return Err(SocketBindError::Retired);
    }
    assert!(facts.port.is_none(), "serialized netlink bind raced itself");
    facts.port = Some(port);
    Ok(port)
}

fn bind_netlink(private: &AnyOpaque, address: SocketAddress) -> Result<(), SocketBindError> {
    let file = netlink_private(private);
    let _operation = file.operation.lock();
    let SocketAddress::Netlink { port, groups } = address else {
        return Err(SocketBindError::AddressUnavailable);
    };
    if port != 0 || groups != 0 {
        return Err(SocketBindError::AddressUnavailable);
    }
    if file.transport.facts.lock().port.is_some() {
        return Err(SocketBindError::AlreadyBound);
    }
    ensure_bound(file).map(|_| ())
}

fn query_local(
    private: &AnyOpaque,
    sink: &mut dyn SocketAddressSink,
) -> Result<(), SocketQueryError> {
    let address = {
        let facts = netlink_private(private).transport.facts.lock();
        if facts.retired {
            return Err(SocketQueryError::Retired);
        }
        SocketAddress::Netlink {
            port: facts.port.unwrap_or(0),
            groups: 0,
        }
    };
    sink.copy_address(Some(address))
        .map_err(SocketQueryError::Copy)
}

fn send_netlink(
    private: &AnyOpaque,
    request: SocketSendRequest<'_>,
) -> Result<usize, SocketSendError> {
    let SocketSendRequest::Datagram {
        destination,
        payload,
        ..
    } = request
    else {
        return Err(SocketSendError::Unsupported);
    };
    if let Some(destination) = destination {
        match destination {
            SocketAddress::Netlink { port: 0, groups: 0 } => {},
            SocketAddress::Netlink { .. } => return Err(SocketSendError::InvalidDestination),
            _ => return Err(SocketSendError::InvalidDestination),
        }
    }
    let file = netlink_private(private);
    let send_budget = {
        let facts = file.transport.facts.lock();
        if facts.retired {
            return Err(SocketSendError::Retired);
        }
        facts.send_budget
    };
    let bytes = payload
        .bytes(send_budget.min(NETLINK_REQUEST_MAX_BYTES))
        .map_err(SocketSendError::Copy)?;
    let parsed = codec::parse_datagram(bytes);
    let local_port = {
        let _operation = file.operation.lock();
        ensure_bound(file).map_err(|error| match error {
            SocketBindError::Retired => SocketSendError::Retired,
            SocketBindError::ResourceExhausted => SocketSendError::ResourceExhausted,
            _ => SocketSendError::InvalidState,
        })?
    };

    // Owner snapshots and Linux reply materialization may allocate, and must
    // not run inside the transport sequencing window. The final short window
    // below only chooses already-owned results and commits their queue order.
    let prepared = parsed
        .requests
        .iter()
        .map(|request| {
            let replies = codec::reply_for_request(file.protocol, request, local_port);
            let reply_bytes = replies
                .iter()
                .try_fold(0usize, |total, reply| total.checked_add(reply.len()));
            let invalid_datagram = replies
                .iter()
                .any(|reply| reply.len() > NETLINK_REPLY_DATAGRAM_MAX_BYTES);
            let no_buffer = codec::no_buffer_reply(request, local_port);
            (replies, reply_bytes, invalid_datagram, no_buffer)
        })
        .collect::<Vec<_>>();
    let maximum_added = prepared.iter().try_fold(0usize, |total, entry| {
        total.checked_add(entry.0.len().max(1))
    });
    let Some(maximum_added) = maximum_added else {
        return Err(SocketSendError::NoBufferSpace);
    };
    let mut added = Vec::with_capacity(maximum_added);
    let mut added_bytes = 0usize;

    let operation = file.operation.lock();

    let (current_bytes, receive_budget) = {
        let facts = file.transport.facts.lock();
        if facts.retired {
            return Err(SocketSendError::Retired);
        }
        if bytes.len() > facts.send_budget {
            return Err(SocketSendError::Copy(SysError::MessageTooLong));
        }
        let current = facts.pending.as_ref().expect("live netlink queue missing");
        (current.bytes, facts.receive_budget)
    };
    let minimum = parsed
        .requests
        .len()
        .checked_mul(codec::MINIMUM_ERROR_REPLY_BYTES)
        .ok_or(SocketSendError::NoBufferSpace)?;
    if current_bytes
        .checked_add(minimum)
        .is_none_or(|bytes| bytes > receive_budget)
    {
        return Err(SocketSendError::NoBufferSpace);
    }

    for (index, (replies, reply_bytes, invalid_datagram, no_buffer)) in
        prepared.into_iter().enumerate()
    {
        let later_minimum = (parsed.requests.len() - index - 1)
            .checked_mul(codec::MINIMUM_ERROR_REPLY_BYTES)
            .ok_or(SocketSendError::NoBufferSpace)?;
        let Some(reply_bytes) = reply_bytes else {
            return Err(SocketSendError::NoBufferSpace);
        };
        let required = current_bytes
            .checked_add(added_bytes)
            .and_then(|bytes| bytes.checked_add(reply_bytes))
            .and_then(|bytes| bytes.checked_add(later_minimum));
        if invalid_datagram || required.is_none_or(|bytes| bytes > receive_budget) {
            added_bytes = added_bytes
                .checked_add(no_buffer.len())
                .ok_or(SocketSendError::NoBufferSpace)?;
            added.push(no_buffer);
        } else {
            added_bytes = added_bytes
                .checked_add(reply_bytes)
                .ok_or(SocketSendError::NoBufferSpace)?;
            added.extend(replies);
        }
    }

    let growth = {
        let facts = file.transport.facts.lock();
        if facts.retired {
            return Err(SocketSendError::Retired);
        }
        facts
            .pending
            .as_ref()
            .expect("live netlink queue missing")
            .required_growth(added.len())
    };
    // The sole sender mutex makes queue length monotonic-decreasing while the
    // replacement capacity is prepared. Allocate only on geometric growth,
    // then move existing Arc entries under the short transport guard.
    let mut replacement = growth.map(VecDeque::with_capacity);
    let mut facts = file.transport.facts.lock();
    if facts.retired {
        return Err(SocketSendError::Retired);
    }
    assert!(
        current_bytes
            .checked_add(added_bytes)
            .is_some_and(|bytes| bytes <= facts.receive_budget)
    );
    let pending = facts.pending.as_mut().expect("live netlink queue missing");
    if let Some(mut grown) = replacement.take() {
        grown.append(&mut pending.datagrams);
        pending.datagrams = grown;
    }
    pending.datagrams.extend(added);
    pending.bytes = pending
        .bytes
        .checked_add(added_bytes)
        .expect("admitted netlink pending bytes overflowed");
    drop(facts);
    drop(operation);
    file.source.invalidate();
    Ok(bytes.len())
}

fn receive_netlink(
    private: &AnyOpaque,
    request: SocketReceiveRequest<'_>,
) -> Result<SocketReceiveOutcome, SocketReceiveError> {
    let SocketReceiveRequest::Datagram { sink, flags } = request else {
        return Err(SocketReceiveError::Unsupported);
    };
    let file = netlink_private(private);
    let datagram = {
        let mut facts = file.transport.facts.lock();
        if facts.retired {
            return Err(SocketReceiveError::Retired);
        }
        let pending = facts.pending.as_mut().expect("live netlink queue missing");
        if flags.peek {
            pending
                .datagrams
                .front()
                .cloned()
                .ok_or(SocketReceiveError::WouldBlock)?
        } else {
            let datagram = pending
                .datagrams
                .pop_front()
                .ok_or(SocketReceiveError::WouldBlock)?;
            pending.bytes = pending
                .bytes
                .checked_sub(datagram.len())
                .expect("netlink pending byte count underflowed");
            datagram
        }
    };
    if !flags.peek {
        file.source.invalidate();
    }
    let packet_length = datagram.len();
    let copied = sink
        .copy_datagram(&datagram, SocketAddress::Netlink { port: 0, groups: 0 })
        .map_err(SocketReceiveError::Copy)?;
    Ok(SocketReceiveOutcome::datagram(copied, packet_length))
}

fn mutate_option(
    private: &AnyOpaque,
    mutation: SocketOptionMutation,
) -> Result<(), SocketOptionError> {
    let file = netlink_private(private);
    let operation = file.operation.lock();
    let mut facts = file.transport.facts.lock();
    if facts.retired {
        return Err(SocketOptionError::Retired);
    }
    let invalidate = match mutation {
        SocketOptionMutation::SendBuffer(value) => {
            facts.send_budget = value.clamp(MINIMUM_BUDGET, NETLINK_REQUEST_MAX_BYTES);
            false
        },
        SocketOptionMutation::ReceiveBuffer(value) => {
            facts.receive_budget = value.clamp(MINIMUM_BUDGET, NETLINK_PENDING_REPLY_MAX_BYTES);
            true
        },
        _ => return Err(SocketOptionError::Unsupported),
    };
    drop(facts);
    drop(operation);
    if invalidate {
        file.source.invalidate();
    }
    Ok(())
}

fn accepting(private: &AnyOpaque) -> Result<bool, SocketQueryError> {
    if netlink_private(private).transport.facts.lock().retired {
        Err(SocketQueryError::Retired)
    } else {
        Ok(false)
    }
}

fn poll_netlink(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    netlink_private(private)
        .source
        .poll(request, |transport, interests| {
            let facts = transport.facts.lock();
            if facts.retired {
                return Ok(PollEvent::HANG_UP);
            }
            let pending = facts.pending.as_ref().expect("live netlink queue missing");
            let mut events = PollEvent::empty();
            if interests.contains(PollEvent::READABLE) && !pending.datagrams.is_empty() {
                events |= PollEvent::READABLE;
            }
            if interests.contains(PollEvent::WRITABLE)
                && pending
                    .bytes
                    .checked_add(codec::MINIMUM_ERROR_REPLY_BYTES)
                    .is_some_and(|required| required <= facts.receive_budget)
            {
                events |= PollEvent::WRITABLE;
            }
            Ok(events)
        })
}

fn retire_transport(protocol: NetlinkProtocol, source: &SocketPollSource<Arc<NetlinkTransport>>) {
    let retired = source.retire(|transport| {
        let mut facts = transport.facts.lock();
        assert!(!facts.retired, "netlink transport retired twice");
        facts.retired = true;
        let port = facts.port.take();
        let pending = facts.pending.take();
        (port, pending)
    });
    let Some((port, pending)) = retired else {
        return;
    };
    if let Some(port) = port {
        registry(protocol).lock().release(port);
    }
    drop(pending);
}

fn final_release(private: &AnyOpaque, _reason: SocketReleaseReason) {
    let file = netlink_private(private);
    retire_transport(file.protocol, &file.source);
}

pub(super) static NETLINK_ROUTE_SOCKET_OPS: SocketOps = SocketOps {
    io: SocketIoOps::Datagram {
        socket_type: SocketType::NetlinkRoute,
        send: send_netlink,
        receive: receive_netlink,
    },
    create: Some(prepare_route),
    create_pair: None,
    bind: Some(bind_netlink),
    listen: None,
    connect: None,
    accept: None,
    shutdown: None,
    local_address: Some(query_local),
    peer_address: None,
    accepting,
    query_option: None,
    mutate_option: Some(mutate_option),
    detach_ipv4_extended_error: None,
    poll: poll_netlink,
    final_release,
};

pub(super) static NETLINK_SOCK_DIAG_SOCKET_OPS: SocketOps = SocketOps {
    io: SocketIoOps::Datagram {
        socket_type: SocketType::NetlinkSockDiag,
        send: send_netlink,
        receive: receive_netlink,
    },
    create: Some(prepare_sock_diag),
    create_pair: None,
    bind: Some(bind_netlink),
    listen: None,
    connect: None,
    accept: None,
    shutdown: None,
    local_address: Some(query_local),
    peer_address: None,
    accepting,
    query_option: None,
    mutate_option: Some(mutate_option),
    detach_ipv4_extended_error: None,
    poll: poll_netlink,
    final_release,
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use anemone_abi::net::linux::{NLM_F_ACK, NLM_F_REQUEST, NlMsgHdr, RTM_NEWLINK};
    use zerocopy::IntoBytes;

    use crate::fs::{
        iomux::{PollObserver, PollRoute},
        socket::{
            SocketDatagramSendOperation, SocketOptionMutation, SocketReceiveFlags,
            SocketReceiveSink, SocketSendPayload,
        },
    };

    struct CountingObserver(AtomicUsize);

    impl PollObserver for CountingObserver {
        fn notify(&self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn route(observer: &Arc<CountingObserver>) -> PollRoute {
        let erased: Arc<dyn PollObserver> = observer.clone();
        let route = PollRoute::new(&erased);
        drop(erased);
        route
    }

    struct Payload(Vec<u8>);

    impl SocketSendPayload for Payload {
        fn bytes(&mut self, maximum: usize) -> Result<&[u8], SysError> {
            if self.0.len() > maximum {
                return Err(SysError::MessageTooLong);
            }
            Ok(&self.0)
        }
    }

    struct Capture {
        bytes: Vec<u8>,
        capacity: usize,
    }

    struct Fault;

    impl SocketReceiveSink for Fault {
        fn copy_datagram(
            &mut self,
            _payload: &[u8],
            _peer: SocketAddress,
        ) -> Result<usize, SysError> {
            Err(SysError::InvalidArgument)
        }
    }

    fn request(kind: u16, flags: u16, sequence: u32, payload: &[u8]) -> Vec<u8> {
        let header = NlMsgHdr {
            nlmsg_len: (size_of::<NlMsgHdr>() + payload.len()) as u32,
            nlmsg_type: kind,
            nlmsg_flags: flags,
            nlmsg_seq: sequence,
            nlmsg_pid: 0,
        };
        let mut bytes = header.as_bytes().to_vec();
        bytes.extend_from_slice(payload);
        bytes.resize((bytes.len() + 3) & !3, 0);
        bytes
    }

    fn send_bytes(private: &AnyOpaque, bytes: Vec<u8>) -> Result<usize, SocketSendError> {
        let mut payload = Payload(bytes);
        let mut operation = SocketDatagramSendOperation::new();
        send_netlink(
            private,
            SocketSendRequest::Datagram {
                destination: None,
                payload: &mut payload,
                operation: &mut operation,
            },
        )
    }

    fn receive_capture(private: &AnyOpaque) -> Result<Capture, SocketReceiveError> {
        let mut capture = Capture {
            bytes: Vec::new(),
            capacity: NETLINK_REPLY_DATAGRAM_MAX_BYTES,
        };
        receive_netlink(
            private,
            SocketReceiveRequest::Datagram {
                sink: &mut capture,
                flags: SocketReceiveFlags { peek: false },
            },
        )?;
        Ok(capture)
    }

    impl SocketReceiveSink for Capture {
        fn copy_datagram(
            &mut self,
            payload: &[u8],
            peer: SocketAddress,
        ) -> Result<usize, SysError> {
            assert_eq!(peer, SocketAddress::Netlink { port: 0, groups: 0 });
            let copied = payload.len().min(self.capacity);
            self.bytes.extend_from_slice(&payload[..copied]);
            Ok(copied)
        }
    }

    #[kunit]
    fn autobind_error_peek_consume_and_final_release_share_one_transport() {
        let SocketPreparation { private, creation } = prepare_route().unwrap();
        creation.commit();
        let request = NlMsgHdr {
            nlmsg_len: 32,
            nlmsg_type: RTM_NEWLINK,
            nlmsg_flags: NLM_F_REQUEST | NLM_F_ACK,
            nlmsg_seq: 17,
            nlmsg_pid: 0,
        };
        let mut bytes = request.as_bytes().to_vec();
        bytes.resize(32, 0);
        let mut payload = Payload(bytes);
        let mut operation = SocketDatagramSendOperation::new();
        assert_eq!(
            send_netlink(
                &private,
                SocketSendRequest::Datagram {
                    destination: None,
                    payload: &mut payload,
                    operation: &mut operation,
                },
            ),
            Ok(32)
        );

        let mut local = None;
        struct Address<'a>(&'a mut Option<SocketAddress>);
        impl SocketAddressSink for Address<'_> {
            fn copy_address(&mut self, address: Option<SocketAddress>) -> Result<(), SysError> {
                *self.0 = address;
                Ok(())
            }
        }
        query_local(&private, &mut Address(&mut local)).unwrap();
        assert!(matches!(
            local,
            Some(SocketAddress::Netlink {
                port: 1..,
                groups: 0
            })
        ));

        let mut peek = Capture {
            bytes: Vec::new(),
            capacity: 4,
        };
        assert_eq!(
            receive_netlink(
                &private,
                SocketReceiveRequest::Datagram {
                    sink: &mut peek,
                    flags: SocketReceiveFlags { peek: true },
                },
            ),
            Ok(SocketReceiveOutcome::datagram(4, 36))
        );
        let mut consume = Capture {
            bytes: Vec::new(),
            capacity: 36,
        };
        assert_eq!(
            receive_netlink(
                &private,
                SocketReceiveRequest::Datagram {
                    sink: &mut consume,
                    flags: SocketReceiveFlags { peek: false },
                },
            ),
            Ok(SocketReceiveOutcome::datagram(36, 36))
        );
        assert_eq!(
            i32::from_ne_bytes(consume.bytes[16..20].try_into().unwrap()),
            -1
        );
        assert_eq!(
            receive_netlink(
                &private,
                SocketReceiveRequest::Datagram {
                    sink: &mut consume,
                    flags: SocketReceiveFlags { peek: false },
                },
            ),
            Err(SocketReceiveError::WouldBlock)
        );

        final_release(&private, SocketReleaseReason::FinalRelease);
        assert_eq!(
            query_local(&private, &mut Address(&mut local)),
            Err(SocketQueryError::Retired)
        );
    }

    #[kunit]
    fn receive_budget_growth_invalidates_registered_writable_waiter() {
        if NETLINK_PENDING_REPLY_MAX_BYTES == MINIMUM_BUDGET {
            return;
        }
        let SocketPreparation { private, creation } = prepare_route().unwrap();
        creation.commit();
        let file = netlink_private(&private);
        {
            let mut facts = file.transport.facts.lock();
            facts.receive_budget = MINIMUM_BUDGET;
            let pending = facts.pending.as_mut().unwrap();
            let occupied = MINIMUM_BUDGET - codec::MINIMUM_ERROR_REPLY_BYTES + 1;
            pending.datagrams.push_back(Arc::from(vec![0; occupied]));
            pending.bytes = occupied;
        }
        let observer = Arc::new(CountingObserver(AtomicUsize::new(0)));
        let poll_route = route(&observer);
        assert_eq!(
            poll_netlink(
                &private,
                &PollRequest::register_with_route(PollEvent::WRITABLE, &poll_route),
            )
            .unwrap(),
            PollRegisterResult::Subscribed(PollEvent::empty())
        );

        mutate_option(
            &private,
            SocketOptionMutation::ReceiveBuffer(
                (MINIMUM_BUDGET + codec::MINIMUM_ERROR_REPLY_BYTES)
                    .min(NETLINK_PENDING_REPLY_MAX_BYTES),
            ),
        )
        .unwrap();
        assert_eq!(observer.0.load(Ordering::Acquire), 1);
        assert_eq!(
            poll_netlink(&private, &PollRequest::snapshot(PollEvent::WRITABLE)).unwrap(),
            PollRegisterResult::Ready(PollEvent::WRITABLE)
        );
        final_release(&private, SocketReleaseReason::FinalRelease);
    }

    #[kunit]
    fn creation_rollback_protocol_ports_and_copy_fault_keep_one_lifecycle() {
        let SocketPreparation { private, creation } = prepare_route().unwrap();
        drop(creation);
        let mut address = None;
        struct Address<'a>(&'a mut Option<SocketAddress>);
        impl SocketAddressSink for Address<'_> {
            fn copy_address(&mut self, value: Option<SocketAddress>) -> Result<(), SysError> {
                *self.0 = value;
                Ok(())
            }
        }
        assert_eq!(
            query_local(&private, &mut Address(&mut address)),
            Err(SocketQueryError::Retired)
        );

        let SocketPreparation {
            private: route_private,
            creation: route_creation,
        } = prepare_route().unwrap();
        route_creation.commit();
        let SocketPreparation {
            private: diag_private,
            creation: diag_creation,
        } = prepare_sock_diag().unwrap();
        diag_creation.commit();
        let route_port = ensure_bound(netlink_private(&route_private)).unwrap();
        let diag_port = ensure_bound(netlink_private(&diag_private)).unwrap();
        assert!(ROUTE_PORTS.lock().live.contains(&route_port));
        assert!(DIAG_PORTS.lock().live.contains(&diag_port));

        let mutation = request(
            RTM_NEWLINK,
            NLM_F_REQUEST | NLM_F_ACK,
            73,
            &[0; size_of::<anemone_abi::net::linux::IfInfoMsg>()],
        );
        send_bytes(&route_private, mutation.clone()).unwrap();
        let mut fault = Fault;
        assert!(matches!(
            receive_netlink(
                &route_private,
                SocketReceiveRequest::Datagram {
                    sink: &mut fault,
                    flags: SocketReceiveFlags { peek: false },
                },
            ),
            Err(SocketReceiveError::Copy(SysError::InvalidArgument))
        ));
        assert!(matches!(
            receive_capture(&route_private),
            Err(SocketReceiveError::WouldBlock)
        ));

        send_bytes(&route_private, mutation).unwrap();
        assert!(matches!(
            receive_netlink(
                &route_private,
                SocketReceiveRequest::Datagram {
                    sink: &mut fault,
                    flags: SocketReceiveFlags { peek: true },
                },
            ),
            Err(SocketReceiveError::Copy(SysError::InvalidArgument))
        ));
        assert_eq!(
            i32::from_ne_bytes(
                receive_capture(&route_private).unwrap().bytes[16..20]
                    .try_into()
                    .unwrap()
            ),
            -(anemone_abi::errno::EPERM as i32)
        );

        final_release(&route_private, SocketReleaseReason::FinalRelease);
        assert!(!ROUTE_PORTS.lock().live.contains(&route_port));
        assert!(DIAG_PORTS.lock().live.contains(&diag_port));
        final_release(&diag_private, SocketReleaseReason::FinalRelease);
        assert!(!DIAG_PORTS.lock().live.contains(&diag_port));
    }

    #[kunit]
    fn multi_request_capacity_publishes_complete_results_in_order() {
        use anemone_abi::net::linux::{IfAddrMsg, RTM_GETADDR, RTM_NEWADDR};

        let SocketPreparation { private, creation } = prepare_route().unwrap();
        creation.commit();
        let mut datagram = request(
            RTM_GETADDR,
            NLM_F_REQUEST | anemone_abi::net::linux::NLM_F_DUMP,
            81,
            IfAddrMsg::default().as_bytes(),
        );
        datagram.extend_from_slice(&request(
            RTM_NEWLINK,
            NLM_F_REQUEST | NLM_F_ACK,
            82,
            &[0; size_of::<anemone_abi::net::linux::IfInfoMsg>()],
        ));
        send_bytes(&private, datagram.clone()).unwrap();
        let mut kinds = Vec::new();
        loop {
            let reply = receive_capture(&private).unwrap().bytes;
            let kind = u16::from_ne_bytes(reply[4..6].try_into().unwrap());
            let sequence = u32::from_ne_bytes(reply[8..12].try_into().unwrap());
            kinds.push((kind, sequence));
            if sequence == 82 {
                assert_eq!(kind, anemone_abi::net::linux::NLMSG_ERROR);
                assert_eq!(
                    i32::from_ne_bytes(reply[16..20].try_into().unwrap()),
                    -(anemone_abi::errno::EPERM as i32)
                );
                break;
            }
        }
        assert!(kinds.iter().any(|entry| *entry == (RTM_NEWADDR, 81)));
        assert!(
            kinds
                .iter()
                .any(|entry| { *entry == (anemone_abi::net::linux::NLMSG_DONE, 81) })
        );

        {
            let mut facts = netlink_private(&private).transport.facts.lock();
            facts.receive_budget = codec::MINIMUM_ERROR_REPLY_BYTES * 2;
        }
        send_bytes(&private, datagram).unwrap();
        for expected_sequence in [81, 82] {
            let reply = receive_capture(&private).unwrap().bytes;
            assert_eq!(
                u16::from_ne_bytes(reply[4..6].try_into().unwrap()),
                anemone_abi::net::linux::NLMSG_ERROR
            );
            assert_eq!(
                u32::from_ne_bytes(reply[8..12].try_into().unwrap()),
                expected_sequence
            );
        }
        assert!(matches!(
            receive_capture(&private),
            Err(SocketReceiveError::WouldBlock)
        ));
        final_release(&private, SocketReleaseReason::FinalRelease);
    }
}
