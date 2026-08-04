//! Unix seqpacket record direction, transaction, and readiness owner.

use crate::{
    fs::socket::{
        SocketReceiveError, SocketReceiveOutcome, SocketReceiveRequest, SocketSendError,
        SocketSendRequest, SocketShutdown, SocketShutdownError, SocketStreamDestination,
        SocketWait,
    },
    kconfig_defs::{
        UNIX_SEQPACKET_DIRECTION_CAPACITY_BYTES, UNIX_SEQPACKET_DIRECTION_MAX_RECORDS,
        UNIX_SEQPACKET_MAX_PAYLOAD_BYTES,
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::{
    EndpointAccessError, EndpointAssociation, EndpointName, EndpointSide, EndpointState,
    UnixPollRoute, endpoint, replacement_poll_routes,
};

static_assert!(
    UNIX_SEQPACKET_MAX_PAYLOAD_BYTES > 0,
    "unix_seqpacket_max_payload_bytes must be non-zero"
);
static_assert!(
    UNIX_SEQPACKET_DIRECTION_CAPACITY_BYTES >= UNIX_SEQPACKET_MAX_PAYLOAD_BYTES,
    "seqpacket payload maximum must fit its direction byte budget"
);
static_assert!(
    UNIX_SEQPACKET_DIRECTION_MAX_RECORDS > 0,
    "unix_seqpacket_direction_max_records must be non-zero"
);
static_assert!(
    UNIX_SEQPACKET_DIRECTION_MAX_RECORDS < usize::MAX,
    "seqpacket record budget must leave room for VecDeque bookkeeping"
);

#[derive(Debug)]
struct RecordDirection {
    /// Sole record-order truth for this writer-to-reader direction.
    records: VecDeque<Vec<u8>>,
    bytes: usize,
    writer_open: bool,
    reader_open: bool,
}

impl RecordDirection {
    fn new() -> Self {
        Self {
            records: VecDeque::with_capacity(UNIX_SEQPACKET_DIRECTION_MAX_RECORDS),
            bytes: 0,
            writer_open: true,
            reader_open: true,
        }
    }

    fn available_bytes(&self) -> usize {
        assert!(self.bytes <= UNIX_SEQPACKET_DIRECTION_CAPACITY_BYTES);
        UNIX_SEQPACKET_DIRECTION_CAPACITY_BYTES - self.bytes
    }

    fn available_records(&self) -> usize {
        assert!(self.records.len() <= UNIX_SEQPACKET_DIRECTION_MAX_RECORDS);
        UNIX_SEQPACKET_DIRECTION_MAX_RECORDS - self.records.len()
    }
}

#[derive(Debug)]
struct RecordConnectionState {
    directions: [RecordDirection; 2],
    pub(super) routes: [Arc<Vec<UnixPollRoute>>; 2],
}

impl RecordConnectionState {
    fn new() -> Self {
        Self {
            directions: [RecordDirection::new(), RecordDirection::new()],
            routes: [Arc::new(Vec::new()), Arc::new(Vec::new())],
        }
    }

    fn revents(&self, side: EndpointSide, interests: PollEvent) -> PollEvent {
        let index = side.index();
        let incoming = &self.directions[side.peer().index()];
        let outgoing = &self.directions[index];
        let receive_terminal = !incoming.writer_open || !incoming.reader_open;
        let send_terminal = !outgoing.writer_open || !outgoing.reader_open;
        let mut events = PollEvent::empty();
        if interests.contains(PollEvent::READABLE)
            && (!incoming.records.is_empty() || receive_terminal)
        {
            events |= PollEvent::READABLE;
        }
        if interests.contains(PollEvent::WRITABLE)
            && (send_terminal
                || (outgoing.available_records() > 0 && outgoing.available_bytes() > 0))
        {
            events |= PollEvent::WRITABLE;
        }
        if interests.contains(PollEvent::READ_HANG_UP) && receive_terminal {
            events |= PollEvent::READ_HANG_UP;
        }
        if receive_terminal && send_terminal {
            events |= PollEvent::HANG_UP;
        }
        events
    }
}

#[derive(Debug)]
pub(in crate::fs::socket::unix) struct UnixSeqpacketConnection {
    /// Direction-local record truth and route publication.
    state: SpinLock<RecordConnectionState>,
    /// These gates serialize one head-record receive or one whole-record send
    /// without holding a spinlock during fallible user copy.
    read_operations: [Mutex<()>; 2],
    write_operations: [Mutex<()>; 2],
    pub(super) names: [Arc<EndpointName>; 2],
}

impl UnixSeqpacketConnection {
    pub(super) fn new(names: [Arc<EndpointName>; 2]) -> Arc<Self> {
        Arc::new(Self {
            state: SpinLock::new(RecordConnectionState::new()),
            read_operations: [Mutex::new(()), Mutex::new(())],
            write_operations: [Mutex::new(()), Mutex::new(())],
            names,
        })
    }

    pub(super) fn install_routes(&self, side: EndpointSide, routes: Arc<Vec<UnixPollRoute>>) {
        let mut state = self.state.lock();
        let slot = &mut state.routes[side.index()];
        assert!(slot.is_empty(), "Unix seqpacket routes installed twice");
        *slot = routes;
    }
}

fn notify_routes(
    routes: &Arc<Vec<UnixPollRoute>>,
    changed: Option<PollEvent>,
    reason: &'static str,
) {
    let mut candidates = 0usize;
    for entry in routes.iter() {
        if changed.is_none_or(|changed| entry.interests.intersects(changed)) {
            entry.route.notify();
            candidates += 1;
        }
    }
    if candidates > 0 {
        kdebugln!(
            "unix seqpacket: issued {} route hints reason={}",
            candidates,
            reason,
        );
    }
}

fn validate_connection(
    state: &EndpointState,
    connection: &Arc<UnixSeqpacketConnection>,
    side: EndpointSide,
) -> Result<(), EndpointAccessError> {
    match &state.association {
        EndpointAssociation::Connected {
            connection: super::UnixConnection::Seqpacket(current),
            side: current_side,
        } if Arc::ptr_eq(current, connection) && *current_side == side => Ok(()),
        EndpointAssociation::Unconnected => Err(EndpointAccessError::Unconnected),
        EndpointAssociation::Listening(_) => Err(EndpointAccessError::InvalidState),
        EndpointAssociation::Retired => Err(EndpointAccessError::Retired),
        EndpointAssociation::Connected { .. } => Err(EndpointAccessError::InvalidState),
    }
}

#[derive(Debug, Opaque)]
struct SeqpacketSendWaitSource {
    endpoint: Arc<super::UnixEndpointCore>,
    payload_len: usize,
}

fn send_wait_ready(direction: &RecordDirection, payload_len: usize) -> bool {
    if payload_len == 0 || payload_len > UNIX_SEQPACKET_MAX_PAYLOAD_BYTES {
        return true;
    }
    !direction.writer_open
        || !direction.reader_open
        || (direction.available_records() > 0 && direction.available_bytes() >= payload_len)
}

fn poll_seqpacket_send_wait(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let source = private
        .cast::<SeqpacketSendWaitSource>()
        .expect("Unix seqpacket send wait used without its source");
    let endpoint = &source.endpoint;

    loop {
        let (connection, side) = match &endpoint.state.lock().association {
            EndpointAssociation::Connected {
                connection: super::UnixConnection::Seqpacket(connection),
                side,
            } => (connection.clone(), *side),
            EndpointAssociation::Unconnected
            | EndpointAssociation::Listening(_)
            | EndpointAssociation::Connected { .. }
            | EndpointAssociation::Retired => {
                return Ok(PollRegisterResult::Ready(PollEvent::WRITABLE));
            },
        };
        let expected = connection.state.lock().routes[side.index()].clone();

        let Some(route) = request.route() else {
            let endpoint_state = endpoint.state.lock();
            if validate_connection(&endpoint_state, &connection, side).is_err() {
                return Ok(PollRegisterResult::Ready(PollEvent::WRITABLE));
            }
            let state = connection.state.lock();
            let ready = send_wait_ready(&state.directions[side.index()], source.payload_len);
            return Ok(PollRegisterResult::Ready(
                ready
                    .then_some(PollEvent::WRITABLE)
                    .unwrap_or_else(PollEvent::empty),
            ));
        };

        let replacement = replacement_poll_routes(&expected, route, request.interests());
        let (previous, ready) = {
            let endpoint_state = endpoint.state.lock();
            if validate_connection(&endpoint_state, &connection, side).is_err() {
                return Ok(PollRegisterResult::Ready(PollEvent::WRITABLE));
            }
            let mut state = connection.state.lock();
            if !Arc::ptr_eq(&state.routes[side.index()], &expected) {
                continue;
            }
            let previous = core::mem::replace(&mut state.routes[side.index()], replacement);
            let ready = send_wait_ready(&state.directions[side.index()], source.payload_len);
            (previous, ready)
        };
        drop(previous);
        return Ok(PollRegisterResult::Subscribed(
            ready
                .then_some(PollEvent::WRITABLE)
                .unwrap_or_else(PollEvent::empty),
        ));
    }
}

pub(super) fn prepare_seqpacket_send_wait(private: &AnyOpaque, payload_len: usize) -> SocketWait {
    SocketWait::new(
        AnyOpaque::new(SeqpacketSendWaitSource {
            endpoint: endpoint(private).core.clone(),
            payload_len,
        }),
        poll_seqpacket_send_wait,
    )
}

fn map_access_error(error: EndpointAccessError) -> SocketReceiveError {
    match error {
        EndpointAccessError::Unconnected | EndpointAccessError::InvalidState => {
            SocketReceiveError::InvalidState
        },
        EndpointAccessError::Retired => SocketReceiveError::Retired,
    }
}

fn map_send_access_error(error: EndpointAccessError) -> SocketSendError {
    match error {
        EndpointAccessError::Unconnected => SocketSendError::NotConnected,
        EndpointAccessError::InvalidState => SocketSendError::InvalidState,
        EndpointAccessError::Retired => SocketSendError::Retired,
    }
}

pub(super) fn receive_unix_seqpacket(
    private: &AnyOpaque,
    request: SocketReceiveRequest<'_>,
) -> Result<SocketReceiveOutcome, SocketReceiveError> {
    let SocketReceiveRequest::Seqpacket { sink, flags } = request else {
        return Err(SocketReceiveError::Unsupported);
    };
    // The current Anemone ABI deliberately does not let a zero-length receive
    // observe or consume the head record. Linux differs; the accepted
    // limitation owns the compatibility gap and its exit condition.
    if sink.remaining() == 0 {
        return Ok(SocketReceiveOutcome::seqpacket(0, 0));
    }
    let endpoint = &endpoint(private).core;
    let (connection, side) = endpoint.connected_seqpacket().map_err(map_access_error)?;
    let incoming_index = side.peer().index();
    let _operation = connection.read_operations[side.index()].lock();

    let (record_len, prefix_len) = {
        let endpoint_state = endpoint.state.lock();
        validate_connection(&endpoint_state, &connection, side).map_err(map_access_error)?;
        let state = connection.state.lock();
        let incoming = &state.directions[incoming_index];
        let Some(record) = incoming.records.front() else {
            return if incoming.writer_open && incoming.reader_open {
                Err(SocketReceiveError::WouldBlock)
            } else {
                Ok(SocketReceiveOutcome::seqpacket(0, 0))
            };
        };
        (record.len(), sink.remaining().min(record.len()))
    };
    assert!(record_len > 0 && prefix_len > 0);

    let mut staged = Vec::new();
    staged
        .try_reserve_exact(record_len)
        .map_err(|_| SocketReceiveError::Copy(SysError::OutOfMemory))?;
    {
        let state = connection.state.lock();
        let incoming = &state.directions[incoming_index];
        let record = incoming
            .records
            .front()
            .expect("seqpacket head disappeared under read operation gate");
        assert_eq!(record.len(), record_len);
        staged.extend_from_slice(record);
    }

    sink.copy_exact(&staged[..prefix_len])
        .map_err(SocketReceiveError::Copy)?;

    if flags.peek {
        let endpoint_state = endpoint.state.lock();
        validate_connection(&endpoint_state, &connection, side).map_err(map_access_error)?;
        return Ok(SocketReceiveOutcome::seqpacket(prefix_len, record_len));
    }

    let (reader_routes, writer_routes) = {
        let endpoint_state = endpoint.state.lock();
        validate_connection(&endpoint_state, &connection, side).map_err(map_access_error)?;
        let mut state = connection.state.lock();
        let incoming = &mut state.directions[incoming_index];
        let record = incoming
            .records
            .front()
            .expect("seqpacket head disappeared before receive commit");
        assert_eq!(
            record, &staged,
            "seqpacket head changed before receive commit"
        );
        let record = incoming
            .records
            .pop_front()
            .expect("seqpacket head vanished during receive commit");
        assert_eq!(record.len(), record_len);
        incoming.bytes = incoming
            .bytes
            .checked_sub(record_len)
            .expect("seqpacket receive released more capacity than charged");
        (
            state.routes[side.index()].clone(),
            state.routes[incoming_index].clone(),
        )
    };
    notify_routes(
        &reader_routes,
        Some(PollEvent::READABLE),
        "record receive commit",
    );
    notify_routes(
        &writer_routes,
        Some(PollEvent::WRITABLE),
        "record receive commit",
    );
    Ok(SocketReceiveOutcome::seqpacket(prefix_len, record_len))
}

pub(super) fn send_unix_seqpacket(
    private: &AnyOpaque,
    request: SocketSendRequest<'_>,
) -> Result<usize, SocketSendError> {
    let SocketSendRequest::Seqpacket {
        source,
        destination,
    } = request
    else {
        return Err(SocketSendError::Unsupported);
    };
    let endpoint = &endpoint(private).core;
    if destination == SocketStreamDestination::Present {
        return match endpoint.connected_seqpacket() {
            Ok(_) => Err(SocketSendError::AlreadyConnected),
            Err(EndpointAccessError::Unconnected) => Err(SocketSendError::NotConnected),
            Err(EndpointAccessError::InvalidState) => Err(SocketSendError::InvalidState),
            Err(EndpointAccessError::Retired) => Err(SocketSendError::Retired),
        };
    }
    let (connection, side) = endpoint
        .connected_seqpacket()
        .map_err(map_send_access_error)?;
    let index = side.index();
    let peer_index = side.peer().index();
    let _operation = connection.write_operations[index].lock();

    let payload_len = source.remaining();
    if payload_len > UNIX_SEQPACKET_MAX_PAYLOAD_BYTES {
        return Err(SocketSendError::MessageTooLong);
    }
    if payload_len == 0 {
        // The target deliberately does not publish a distinguishable empty
        // record; zero-length writes are successful no-ops.
        return Ok(0);
    }

    {
        let endpoint_state = endpoint.state.lock();
        validate_connection(&endpoint_state, &connection, side).map_err(map_send_access_error)?;
        let state = connection.state.lock();
        let outgoing = &state.directions[index];
        if !outgoing.writer_open || !outgoing.reader_open {
            return Err(SocketSendError::PeerClosed);
        }
        if outgoing.available_records() == 0 || outgoing.available_bytes() < payload_len {
            return Err(SocketSendError::WouldBlock);
        }
    }

    let mut staged = Vec::new();
    staged
        .try_reserve_exact(payload_len)
        .map_err(|_| SocketSendError::Copy(SysError::OutOfMemory))?;
    staged.resize(payload_len, 0);
    source
        .copy_exact(&mut staged)
        .map_err(SocketSendError::Copy)?;

    let (writer_routes, peer_routes) = {
        let endpoint_state = endpoint.state.lock();
        validate_connection(&endpoint_state, &connection, side).map_err(map_send_access_error)?;
        let mut state = connection.state.lock();
        let outgoing = &mut state.directions[index];
        if !outgoing.writer_open || !outgoing.reader_open {
            return Err(SocketSendError::PeerClosed);
        }
        if outgoing.available_records() == 0 || outgoing.available_bytes() < payload_len {
            return Err(SocketSendError::WouldBlock);
        }
        outgoing.bytes = outgoing
            .bytes
            .checked_add(payload_len)
            .expect("seqpacket send capacity charge overflow");
        outgoing.records.push_back(staged);
        (
            state.routes[index].clone(),
            state.routes[peer_index].clone(),
        )
    };
    notify_routes(
        &writer_routes,
        Some(PollEvent::WRITABLE),
        "record send commit",
    );
    notify_routes(
        &peer_routes,
        Some(PollEvent::READABLE),
        "record send commit",
    );
    Ok(payload_len)
}

pub(super) fn shutdown_unix_seqpacket(
    private: &AnyOpaque,
    how: SocketShutdown,
) -> Result<(), SocketShutdownError> {
    let endpoint = &endpoint(private).core;
    let (connection, side) = endpoint
        .connected_seqpacket()
        .map_err(|error| match error {
            EndpointAccessError::Unconnected | EndpointAccessError::InvalidState => {
                SocketShutdownError::NotConnected
            },
            EndpointAccessError::Retired => SocketShutdownError::Retired,
        })?;
    let index = side.index();
    let incoming_index = side.peer().index();
    let _read_operation = matches!(how, SocketShutdown::Read | SocketShutdown::ReadWrite)
        .then(|| connection.read_operations[index].lock());
    let _write_operation = matches!(how, SocketShutdown::Write | SocketShutdown::ReadWrite)
        .then(|| connection.write_operations[index].lock());
    let (changed, own_routes, peer_routes) = {
        let endpoint_state = endpoint.state.lock();
        validate_connection(&endpoint_state, &connection, side).map_err(|error| match error {
            EndpointAccessError::Unconnected | EndpointAccessError::InvalidState => {
                SocketShutdownError::NotConnected
            },
            EndpointAccessError::Retired => SocketShutdownError::Retired,
        })?;
        let mut state = connection.state.lock();
        let mut changed = false;
        if matches!(how, SocketShutdown::Read | SocketShutdown::ReadWrite) {
            changed |= state.directions[incoming_index].reader_open;
            state.directions[incoming_index].reader_open = false;
        }
        if matches!(how, SocketShutdown::Write | SocketShutdown::ReadWrite) {
            changed |= state.directions[index].writer_open;
            state.directions[index].writer_open = false;
        }
        (
            changed,
            state.routes[index].clone(),
            state.routes[incoming_index].clone(),
        )
    };
    if changed {
        notify_routes(&own_routes, None, "record local shutdown");
        notify_routes(&peer_routes, None, "record peer shutdown");
    }
    Ok(())
}

pub(super) fn poll_connected_unix_seqpacket(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let endpoint = &endpoint(private).core;
    let (connection, side) = match endpoint.connected_seqpacket() {
        Ok(association) => association,
        Err(EndpointAccessError::Unconnected | EndpointAccessError::InvalidState) => {
            return Ok(PollRegisterResult::Unsupported);
        },
        Err(EndpointAccessError::Retired) => {
            return Ok(PollRegisterResult::Ready(PollEvent::HANG_UP));
        },
    };
    let Some(route) = request.route() else {
        let endpoint_state = endpoint.state.lock();
        if validate_connection(&endpoint_state, &connection, side).is_err() {
            return Ok(PollRegisterResult::Ready(PollEvent::HANG_UP));
        }
        return Ok(PollRegisterResult::Ready(
            connection.state.lock().revents(side, request.interests()),
        ));
    };

    loop {
        let expected = {
            let endpoint_state = endpoint.state.lock();
            if validate_connection(&endpoint_state, &connection, side).is_err() {
                return Ok(PollRegisterResult::Ready(PollEvent::HANG_UP));
            }
            connection.state.lock().routes[side.index()].clone()
        };
        let replacement = replacement_poll_routes(&expected, route, request.interests());
        let (previous, events) = {
            let endpoint_state = endpoint.state.lock();
            if validate_connection(&endpoint_state, &connection, side).is_err() {
                return Ok(PollRegisterResult::Ready(PollEvent::HANG_UP));
            }
            let mut state = connection.state.lock();
            if !Arc::ptr_eq(&state.routes[side.index()], &expected) {
                continue;
            }
            let previous = core::mem::replace(&mut state.routes[side.index()], replacement);
            let events = state.revents(side, request.interests());
            (previous, events)
        };
        drop(previous);
        return Ok(PollRegisterResult::Subscribed(events));
    }
}

pub(super) fn retire_connection_endpoint(
    connection: &Arc<UnixSeqpacketConnection>,
    side: EndpointSide,
) {
    let index = side.index();
    let peer_index = side.peer().index();
    let empty_routes = Arc::new(Vec::new());
    let (own_routes, peer_routes) = {
        let mut state = connection.state.lock();
        state.directions[index].writer_open = false;
        state.directions[peer_index].reader_open = false;
        let own_routes = core::mem::replace(&mut state.routes[index], empty_routes);
        let peer_routes = state.routes[peer_index].clone();
        (own_routes, peer_routes)
    };
    notify_routes(&own_routes, None, "record endpoint retirement");
    notify_routes(&peer_routes, None, "record peer final close");
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{
        fs::{
            iomux::PollObserver,
            socket::{
                SocketReadSink, SocketReceiveFlags, SocketReceiveRequest, SocketSendRequest,
                SocketWriteSource,
            },
        },
        syserror::SysError,
    };

    #[derive(Default)]
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

    struct Bytes<'a>(&'a [u8]);

    impl SocketWriteSource for Bytes<'_> {
        fn remaining(&self) -> usize {
            self.0.len()
        }

        fn copy_bytes(&mut self, target: &mut [u8]) -> Result<usize, SysError> {
            let copied = self.0.len().min(target.len());
            target[..copied].copy_from_slice(&self.0[..copied]);
            Ok(copied)
        }
    }

    struct CursorBytes<'a> {
        bytes: &'a [u8],
        offset: usize,
    }

    impl SocketWriteSource for CursorBytes<'_> {
        fn remaining(&self) -> usize {
            self.bytes.len() - self.offset
        }

        fn copy_bytes(&mut self, target: &mut [u8]) -> Result<usize, SysError> {
            let copied = self.remaining().min(target.len());
            target[..copied].copy_from_slice(&self.bytes[self.offset..self.offset + copied]);
            self.offset += copied;
            Ok(copied)
        }
    }

    struct Capture {
        bytes: Vec<u8>,
        capacity: usize,
    }

    impl SocketReadSink for Capture {
        fn remaining(&self) -> usize {
            self.capacity
        }

        fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError> {
            let copied = self.capacity.min(bytes.len());
            self.bytes.extend_from_slice(&bytes[..copied]);
            Ok(copied)
        }
    }

    struct FaultSink {
        capacity: usize,
    }

    impl SocketReadSink for FaultSink {
        fn remaining(&self) -> usize {
            self.capacity
        }

        fn copy_bytes(&mut self, _bytes: &[u8]) -> Result<usize, SysError> {
            Err(SysError::BadAddress)
        }
    }

    fn pair() -> (AnyOpaque, AnyOpaque) {
        let first = super::super::UnixEndpointCore::new_seqpacket();
        let second = super::super::UnixEndpointCore::new_seqpacket();
        let connection = super::super::UnixConnection::new(
            super::super::UnixProfile::Seqpacket,
            [first.name.clone(), second.name.clone()],
        );
        first.install_connection(connection.clone(), EndpointSide::First);
        second.install_connection(connection, EndpointSide::Second);
        (
            super::super::private_from_core(first),
            super::super::private_from_core(second),
        )
    }

    fn send(private: &AnyOpaque, bytes: &[u8]) -> Result<usize, SocketSendError> {
        let mut source = Bytes(bytes);
        send_unix_seqpacket(
            private,
            SocketSendRequest::Seqpacket {
                source: &mut source,
                destination: SocketStreamDestination::Absent,
            },
        )
    }

    fn receive(
        private: &AnyOpaque,
        capacity: usize,
        peek: bool,
    ) -> Result<(SocketReceiveOutcome, Vec<u8>), SocketReceiveError> {
        let mut sink = Capture {
            bytes: Vec::new(),
            capacity,
        };
        let outcome = receive_unix_seqpacket(
            private,
            SocketReceiveRequest::Seqpacket {
                sink: &mut sink,
                flags: SocketReceiveFlags { peek },
            },
        )?;
        Ok((outcome, sink.bytes))
    }

    #[kunit]
    fn record_boundaries_short_peek_and_truncation() {
        let (first, second) = pair();
        assert_eq!(send(&first, b"one"), Ok(3));
        assert_eq!(send(&first, b"second"), Ok(6));

        assert_eq!(
            receive(&second, 2, true),
            Ok((SocketReceiveOutcome::seqpacket(2, 3), b"on".to_vec()))
        );
        assert_eq!(
            receive(&second, 2, false),
            Ok((SocketReceiveOutcome::seqpacket(2, 3), b"on".to_vec()))
        );
        assert_eq!(
            receive(&second, 16, false),
            Ok((SocketReceiveOutcome::seqpacket(6, 6), b"second".to_vec()))
        );
        assert_eq!(
            receive(&second, 16, false),
            Err(SocketReceiveError::WouldBlock)
        );
        super::super::final_release_unix_endpoint(&first);
        super::super::final_release_unix_endpoint(&second);
    }

    #[kunit]
    fn copy_fault_and_zero_destination_preserve_head() {
        let (first, second) = pair();
        assert_eq!(send(&first, b"fault"), Ok(5));

        let mut zero = Capture {
            bytes: Vec::new(),
            capacity: 0,
        };
        assert_eq!(
            receive_unix_seqpacket(
                &second,
                SocketReceiveRequest::Seqpacket {
                    sink: &mut zero,
                    flags: SocketReceiveFlags { peek: false },
                },
            ),
            Ok(SocketReceiveOutcome::seqpacket(0, 0))
        );

        let mut fault = FaultSink { capacity: 2 };
        assert_eq!(
            receive_unix_seqpacket(
                &second,
                SocketReceiveRequest::Seqpacket {
                    sink: &mut fault,
                    flags: SocketReceiveFlags { peek: false },
                },
            ),
            Err(SocketReceiveError::Copy(SysError::BadAddress))
        );
        assert_eq!(
            receive(&second, 16, false),
            Ok((SocketReceiveOutcome::seqpacket(5, 5), b"fault".to_vec()))
        );
        super::super::final_release_unix_endpoint(&first);
        super::super::final_release_unix_endpoint(&second);
    }

    #[kunit]
    fn bounded_capacity_and_source_stability() {
        let (first, second) = pair();
        let oversized = vec![0u8; UNIX_SEQPACKET_MAX_PAYLOAD_BYTES + 1];
        assert_eq!(
            send(&first, &oversized),
            Err(SocketSendError::MessageTooLong)
        );

        let mut source = CursorBytes {
            bytes: b"x",
            offset: 0,
        };
        for _ in 0..UNIX_SEQPACKET_DIRECTION_MAX_RECORDS {
            assert_eq!(
                send_unix_seqpacket(
                    &first,
                    SocketSendRequest::Seqpacket {
                        source: &mut source,
                        destination: SocketStreamDestination::Absent,
                    },
                ),
                Ok(1)
            );
            source.offset = 0;
        }
        assert_eq!(source.offset, 0);
        assert_eq!(
            send_unix_seqpacket(
                &first,
                SocketSendRequest::Seqpacket {
                    source: &mut source,
                    destination: SocketStreamDestination::Absent,
                },
            ),
            Err(SocketSendError::WouldBlock)
        );
        assert_eq!(source.offset, 0);

        assert_eq!(receive(&second, 1, false).unwrap().0.copied(), 1);
        assert_eq!(
            send_unix_seqpacket(
                &first,
                SocketSendRequest::Seqpacket {
                    source: &mut source,
                    destination: SocketStreamDestination::Absent,
                },
            ),
            Ok(1)
        );
        super::super::final_release_unix_endpoint(&first);
        super::super::final_release_unix_endpoint(&second);
    }

    #[kunit]
    fn payload_specific_send_wait_does_not_spin_on_public_writable_hint() {
        let (first, second) = pair();
        assert_eq!(send(&first, b"x"), Ok(1));

        let public =
            poll_connected_unix_seqpacket(&first, &PollRequest::snapshot(PollEvent::WRITABLE))
                .unwrap()
                .expect_ready("seqpacket public writability");
        assert!(public.contains(PollEvent::WRITABLE));

        let wait = prepare_seqpacket_send_wait(&first, UNIX_SEQPACKET_MAX_PAYLOAD_BYTES);
        assert_eq!(
            wait.poll(&PollRequest::snapshot(PollEvent::WRITABLE))
                .unwrap(),
            PollRegisterResult::Ready(PollEvent::empty())
        );

        let observer = Arc::new(CountingObserver::default());
        assert_eq!(
            wait.poll(&PollRequest::register_with_route(
                PollEvent::WRITABLE,
                &route(&observer),
            ))
            .unwrap(),
            PollRegisterResult::Subscribed(PollEvent::empty())
        );
        assert_eq!(receive(&second, 1, false).unwrap().1, b"x");
        assert_eq!(observer.0.load(Ordering::Acquire), 1);
        assert_eq!(
            wait.poll(&PollRequest::snapshot(PollEvent::WRITABLE))
                .unwrap(),
            PollRegisterResult::Ready(PollEvent::WRITABLE)
        );

        super::super::final_release_unix_endpoint(&first);
        super::super::final_release_unix_endpoint(&second);
    }

    #[kunit]
    fn shutdown_preserves_queued_record_and_projects_terminal_events() {
        let (first, second) = pair();
        assert_eq!(send(&first, b"queued"), Ok(6));
        shutdown_unix_seqpacket(&first, SocketShutdown::Write).unwrap();
        assert_eq!(send(&first, b"x"), Err(SocketSendError::PeerClosed));

        let events = poll_connected_unix_seqpacket(
            &second,
            &PollRequest::snapshot(PollEvent::READABLE | PollEvent::READ_HANG_UP),
        )
        .unwrap()
        .expect_ready("seqpacket queued record readiness");
        assert!(events.contains(PollEvent::READABLE | PollEvent::READ_HANG_UP));
        assert_eq!(receive(&second, 16, false).unwrap().1, b"queued");
        assert_eq!(receive(&second, 16, false).unwrap().0.copied(), 0);

        shutdown_unix_seqpacket(&second, SocketShutdown::ReadWrite).unwrap();
        let events = poll_connected_unix_seqpacket(
            &second,
            &PollRequest::snapshot(PollEvent::READABLE | PollEvent::WRITABLE | PollEvent::HANG_UP),
        )
        .unwrap()
        .expect_ready("seqpacket terminal readiness");
        assert!(events.contains(PollEvent::HANG_UP));
        super::super::final_release_unix_endpoint(&first);
        super::super::final_release_unix_endpoint(&second);
    }
}
