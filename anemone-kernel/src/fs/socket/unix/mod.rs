//! Unix stream endpoint, paired connection, and directional byte owners.

use crate::{fs::iomux::PollRoute, prelude::*, utils::any_opaque::AnyOpaque};

use super::{
    SocketOps, SocketPairPreparation, SocketReceiveError, SocketReceiveRequest, SocketSendError,
    SocketSendRequest, SocketStreamReadSink, SocketStreamWriteSource, SocketType,
};

static_assert!(
    UNIX_STREAM_DIRECTION_CAPACITY_BYTES > 0,
    "unix_stream_direction_capacity_bytes must be non-zero"
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EndpointSide {
    First,
    Second,
}

impl EndpointSide {
    const fn index(self) -> usize {
        match self {
            Self::First => 0,
            Self::Second => 1,
        }
    }

    const fn peer(self) -> Self {
        match self {
            Self::First => Self::Second,
            Self::Second => Self::First,
        }
    }
}

#[derive(Clone, Debug)]
struct UnixPollRoute {
    route: PollRoute,
    interests: PollEvent,
}

impl UnixPollRoute {
    fn new(route: &PollRoute, interests: PollEvent) -> Self {
        Self {
            route: route.clone(),
            interests,
        }
    }
}

#[derive(Debug)]
struct StreamDirection {
    /// Sole byte-sequence truth for this writer-to-reader direction.
    bytes: VecDeque<u8>,
    writer_open: bool,
    reader_open: bool,
}

impl StreamDirection {
    fn new() -> Self {
        Self {
            bytes: VecDeque::with_capacity(UNIX_STREAM_DIRECTION_CAPACITY_BYTES),
            writer_open: true,
            reader_open: true,
        }
    }

    fn available(&self) -> usize {
        assert!(self.bytes.len() <= UNIX_STREAM_DIRECTION_CAPACITY_BYTES);
        UNIX_STREAM_DIRECTION_CAPACITY_BYTES - self.bytes.len()
    }
}

#[derive(Debug)]
struct ConnectionState {
    /// Endpoint publication/operation-admission truth. Directional terminal
    /// facts remain in their owning `StreamDirection` entries below.
    endpoint_live: [bool; 2],
    /// Entry N owns bytes written by endpoint N and read by its peer.
    directions: [StreamDirection; 2],
    /// Each endpoint owns the routes used to recheck its combined predicates.
    routes: [Arc<Vec<UnixPollRoute>>; 2],
}

impl ConnectionState {
    fn new() -> Self {
        Self {
            endpoint_live: [true, true],
            directions: [StreamDirection::new(), StreamDirection::new()],
            routes: [Arc::new(Vec::new()), Arc::new(Vec::new())],
        }
    }

    fn revents(&self, side: EndpointSide, interests: PollEvent) -> PollEvent {
        let index = side.index();
        if !self.endpoint_live[index] {
            return PollEvent::HANG_UP;
        }

        let incoming = &self.directions[side.peer().index()];
        let outgoing = &self.directions[index];
        let mut events = PollEvent::empty();
        if interests.contains(PollEvent::READABLE)
            && (!incoming.bytes.is_empty() || !incoming.writer_open)
        {
            events |= PollEvent::READABLE;
        }
        if interests.contains(PollEvent::WRITABLE)
            && outgoing.reader_open
            && outgoing.available() > 0
        {
            events |= PollEvent::WRITABLE;
        }
        // Full peer close is mandatory even when the caller did not request it.
        if !self.endpoint_live[side.peer().index()] {
            events |= PollEvent::HANG_UP;
        }
        events
    }
}

#[derive(Debug)]
struct UnixConnection {
    /// One lock linearizes endpoint retirement, both directional facts, and
    /// route publication. The two direction entries remain the unique owners
    /// of their byte and terminal facts; this lock is not another truth source.
    state: SpinLock<ConnectionState>,
    /// A read gate keeps one selected prefix stable across user copyout.
    read_operations: [Mutex<()>; 2],
    /// A write gate keeps capacity stable while one user prefix is staged.
    write_operations: [Mutex<()>; 2],
}

impl UnixConnection {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: SpinLock::new(ConnectionState::new()),
            read_operations: [Mutex::new(()), Mutex::new(())],
            write_operations: [Mutex::new(()), Mutex::new(())],
        })
    }
}

#[derive(Debug, Opaque)]
struct UnixEndpoint {
    connection: Arc<UnixConnection>,
    side: EndpointSide,
}

fn endpoint(private: &AnyOpaque) -> &UnixEndpoint {
    private
        .cast::<UnixEndpoint>()
        .expect("Unix SocketOps used without Unix endpoint private state")
}

fn prepare_unix_pair() -> Result<SocketPairPreparation, SysError> {
    let connection = UnixConnection::new();
    Ok(SocketPairPreparation {
        first_private: AnyOpaque::new(UnixEndpoint {
            connection: connection.clone(),
            side: EndpointSide::First,
        }),
        second_private: AnyOpaque::new(UnixEndpoint {
            connection,
            side: EndpointSide::Second,
        }),
    })
}

fn replacement_routes(
    current: &Arc<Vec<UnixPollRoute>>,
    route: &PollRoute,
    interests: PollEvent,
) -> Arc<Vec<UnixPollRoute>> {
    let retained = current
        .iter()
        .filter(|entry| !entry.route.is_prunable())
        .count();
    let mut replacement = Vec::with_capacity(retained + 1);
    replacement.extend(
        current
            .iter()
            .filter(|entry| !entry.route.is_prunable())
            .cloned(),
    );
    replacement.push(UnixPollRoute::new(route, interests));
    Arc::new(replacement)
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
            "unix socket: issued {} route hints reason={}",
            candidates,
            reason,
        );
    }
}

fn receive_unix_stream(
    private: &AnyOpaque,
    request: SocketReceiveRequest<'_>,
) -> Result<usize, SocketReceiveError> {
    let SocketReceiveRequest::Stream(sink) = request else {
        return Err(SocketReceiveError::Unsupported);
    };
    if sink.remaining() == 0 {
        return Ok(0);
    }
    let endpoint = endpoint(private);
    let side = endpoint.side;
    let incoming_index = side.peer().index();
    let _operation = endpoint.connection.read_operations[side.index()].lock();

    let staged_len = {
        let state = endpoint.connection.state.lock();
        if !state.endpoint_live[side.index()] {
            return Err(SocketReceiveError::Retired);
        }
        let incoming = &state.directions[incoming_index];
        if incoming.bytes.is_empty() {
            return if incoming.writer_open {
                Err(SocketReceiveError::WouldBlock)
            } else {
                Ok(0)
            };
        }
        sink.remaining().min(incoming.bytes.len())
    };
    assert!(staged_len > 0, "nonempty Unix read selected no bytes");

    let mut staged = Vec::with_capacity(staged_len);
    {
        let state = endpoint.connection.state.lock();
        let incoming = &state.directions[incoming_index];
        assert!(
            incoming.bytes.len() >= staged_len,
            "Unix read prefix changed outside its operation gate"
        );
        staged.extend(incoming.bytes.iter().take(staged_len));
    }

    let copied = sink.copy_bytes(&staged).map_err(SocketReceiveError::Copy)?;
    assert!(
        copied > 0 && copied <= staged.len(),
        "nonempty Unix read copy made invalid progress"
    );

    let (reader_routes, writer_routes) = {
        let mut state = endpoint.connection.state.lock();
        if !state.endpoint_live[side.index()] {
            return Err(SocketReceiveError::Retired);
        }
        let incoming = &mut state.directions[incoming_index];
        for expected in &staged[..copied] {
            let actual = incoming
                .bytes
                .pop_front()
                .expect("Unix staged prefix disappeared before read commit");
            assert_eq!(
                actual, *expected,
                "Unix staged prefix changed before commit"
            );
        }
        (
            state.routes[side.index()].clone(),
            state.routes[incoming_index].clone(),
        )
    };
    notify_routes(&reader_routes, Some(PollEvent::READABLE), "read commit");
    notify_routes(&writer_routes, Some(PollEvent::WRITABLE), "read commit");
    Ok(copied)
}

fn send_unix_stream(
    private: &AnyOpaque,
    request: SocketSendRequest<'_>,
) -> Result<usize, SocketSendError> {
    let SocketSendRequest::Stream(source) = request else {
        return Err(SocketSendError::Unsupported);
    };
    if source.remaining() == 0 {
        return Ok(0);
    }
    let endpoint = endpoint(private);
    let side = endpoint.side;
    let index = side.index();
    let peer_index = side.peer().index();
    let _operation = endpoint.connection.write_operations[index].lock();

    let staged_len = {
        let state = endpoint.connection.state.lock();
        if !state.endpoint_live[index] {
            return Err(SocketSendError::Retired);
        }
        let outgoing = &state.directions[index];
        if !outgoing.reader_open {
            return Err(SocketSendError::PeerClosed);
        }
        if outgoing.available() == 0 {
            return Err(SocketSendError::WouldBlock);
        }
        source.remaining().min(outgoing.available())
    };
    assert!(staged_len > 0, "writable Unix direction selected no bytes");

    let mut staged = Vec::with_capacity(staged_len);
    staged.resize(staged_len, 0);
    let copied = source
        .copy_bytes(&mut staged)
        .map_err(SocketSendError::Copy)?;
    assert!(
        copied > 0 && copied <= staged.len(),
        "nonempty Unix write copy made invalid progress"
    );

    let (writer_routes, peer_routes) = {
        let mut state = endpoint.connection.state.lock();
        if !state.endpoint_live[index] {
            return Err(SocketSendError::Retired);
        }
        let outgoing = &mut state.directions[index];
        if !outgoing.reader_open {
            return Err(SocketSendError::PeerClosed);
        }
        assert!(
            outgoing.available() >= copied,
            "Unix write capacity changed outside its operation gate"
        );
        outgoing.bytes.extend(&staged[..copied]);
        (
            state.routes[index].clone(),
            state.routes[peer_index].clone(),
        )
    };
    notify_routes(&writer_routes, Some(PollEvent::WRITABLE), "write commit");
    notify_routes(&peer_routes, Some(PollEvent::READABLE), "write commit");
    Ok(copied)
}

fn poll_unix_stream(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let endpoint = endpoint(private);
    let side = endpoint.side;
    let Some(route) = request.route() else {
        let state = endpoint.connection.state.lock();
        return Ok(PollRegisterResult::Ready(
            state.revents(side, request.interests()),
        ));
    };

    loop {
        let expected = {
            let state = endpoint.connection.state.lock();
            if !state.endpoint_live[side.index()] {
                return Ok(PollRegisterResult::Ready(
                    state.revents(side, request.interests()),
                ));
            }
            state.routes[side.index()].clone()
        };
        let replacement = replacement_routes(&expected, route, request.interests());
        let (previous, events) = {
            let mut state = endpoint.connection.state.lock();
            if !state.endpoint_live[side.index()] {
                return Ok(PollRegisterResult::Ready(
                    state.revents(side, request.interests()),
                ));
            }
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

fn final_release_unix_stream(private: &AnyOpaque) {
    let endpoint = endpoint(private);
    let side = endpoint.side;
    let index = side.index();
    let peer_index = side.peer().index();
    let empty_routes = Arc::new(Vec::new());
    let (own_routes, peer_routes) = {
        let mut state = endpoint.connection.state.lock();
        assert!(
            state.endpoint_live[index],
            "Unix endpoint final release ran more than once"
        );

        // Withdraw operation/source publication before exposing directional
        // terminal facts. New attempts fail closed; already staged attempts
        // recheck this admission under the same lock before commit.
        state.endpoint_live[index] = false;
        state.directions[index].writer_open = false;
        state.directions[peer_index].reader_open = false;
        let own_routes = core::mem::replace(&mut state.routes[index], empty_routes);
        let peer_routes = state.routes[peer_index].clone();
        (own_routes, peer_routes)
    };

    notify_routes(&own_routes, None, "endpoint retirement");
    notify_routes(&peer_routes, None, "peer final close");
}

pub(super) static UNIX_STREAM_SOCKET_OPS: SocketOps = SocketOps {
    socket_type: SocketType::UnixStream,
    create: None,
    create_pair: Some(prepare_unix_pair),
    bind: None,
    local_address: None,
    send: Some(send_unix_stream),
    receive: Some(receive_unix_stream),
    poll: poll_unix_stream,
    final_release: final_release_unix_stream,
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::fs::iomux::PollObserver;

    fn receive_stream_for_test(
        private: &AnyOpaque,
        sink: &mut dyn SocketStreamReadSink,
    ) -> Result<usize, SocketReceiveError> {
        receive_unix_stream(private, SocketReceiveRequest::Stream(sink))
    }

    fn send_stream_for_test(
        private: &AnyOpaque,
        source: &mut dyn SocketStreamWriteSource,
    ) -> Result<usize, SocketSendError> {
        send_unix_stream(private, SocketSendRequest::Stream(source))
    }

    struct ReadCapture(Vec<u8>);

    impl SocketStreamReadSink for ReadCapture {
        fn remaining(&self) -> usize {
            usize::MAX
        }

        fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError> {
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
    }

    struct WriteBytes<'a>(&'a [u8]);

    impl SocketStreamWriteSource for WriteBytes<'_> {
        fn remaining(&self) -> usize {
            self.0.len()
        }

        fn copy_bytes(&mut self, bytes: &mut [u8]) -> Result<usize, SysError> {
            let copied = bytes.len().min(self.0.len());
            bytes[..copied].copy_from_slice(&self.0[..copied]);
            Ok(copied)
        }
    }

    struct PartialReadCapture {
        bytes: Vec<u8>,
        limit: usize,
    }

    impl SocketStreamReadSink for PartialReadCapture {
        fn remaining(&self) -> usize {
            usize::MAX
        }

        fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError> {
            let copied = bytes.len().min(self.limit);
            self.bytes.extend_from_slice(&bytes[..copied]);
            Ok(copied)
        }
    }

    struct PartialWriteBytes<'a> {
        bytes: &'a [u8],
        limit: usize,
    }

    impl SocketStreamWriteSource for PartialWriteBytes<'_> {
        fn remaining(&self) -> usize {
            self.bytes.len()
        }

        fn copy_bytes(&mut self, dst: &mut [u8]) -> Result<usize, SysError> {
            let copied = dst.len().min(self.bytes.len()).min(self.limit);
            dst[..copied].copy_from_slice(&self.bytes[..copied]);
            Ok(copied)
        }
    }

    struct FaultReadSink;

    impl SocketStreamReadSink for FaultReadSink {
        fn remaining(&self) -> usize {
            usize::MAX
        }

        fn copy_bytes(&mut self, _bytes: &[u8]) -> Result<usize, SysError> {
            Err(SysError::BadAddress)
        }
    }

    struct FaultWriteSource;

    impl SocketStreamWriteSource for FaultWriteSource {
        fn remaining(&self) -> usize {
            1
        }

        fn copy_bytes(&mut self, _bytes: &mut [u8]) -> Result<usize, SysError> {
            Err(SysError::BadAddress)
        }
    }

    struct CountingObserver(AtomicUsize);

    impl CountingObserver {
        fn notifications(&self) -> usize {
            self.0.load(Ordering::Acquire)
        }
    }

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

    #[kunit]
    fn paired_directions_do_not_cross_and_preserve_order() {
        let pair = prepare_unix_pair().unwrap();
        assert_eq!(
            send_stream_for_test(&pair.first_private, &mut WriteBytes(b"first")),
            Ok(5)
        );
        assert_eq!(
            send_stream_for_test(&pair.second_private, &mut WriteBytes(b"second")),
            Ok(6)
        );

        let mut first_read = ReadCapture(Vec::new());
        let mut second_read = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut first_read),
            Ok(6)
        );
        assert_eq!(
            receive_stream_for_test(&pair.second_private, &mut second_read),
            Ok(5)
        );
        assert_eq!(first_read.0, b"second");
        assert_eq!(second_read.0, b"first");
    }

    #[kunit]
    fn capacity_partial_progress_and_final_close_share_direction_truth() {
        let pair = prepare_unix_pair().unwrap();
        let bytes = vec![0x5a; UNIX_STREAM_DIRECTION_CAPACITY_BYTES + 1];
        assert_eq!(
            send_stream_for_test(&pair.first_private, &mut WriteBytes(&bytes)),
            Ok(UNIX_STREAM_DIRECTION_CAPACITY_BYTES)
        );
        assert_eq!(
            send_stream_for_test(&pair.first_private, &mut WriteBytes(b"x")),
            Err(SocketSendError::WouldBlock)
        );

        final_release_unix_stream(&pair.second_private);
        assert_eq!(
            send_stream_for_test(&pair.first_private, &mut WriteBytes(b"x")),
            Err(SocketSendError::PeerClosed)
        );
        let events = poll_unix_stream(
            &pair.first_private,
            &PollRequest::snapshot(PollEvent::READABLE | PollEvent::WRITABLE),
        )
        .unwrap()
        .expect_ready("Unix close KUnit");
        assert!(events.contains(PollEvent::HANG_UP));

        let mut drained = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut drained),
            Ok(0)
        );
    }

    #[kunit]
    fn copy_fault_and_short_copy_commit_only_the_reported_prefix() {
        let pair = prepare_unix_pair().unwrap();

        assert_eq!(
            send_stream_for_test(&pair.first_private, &mut FaultWriteSource),
            Err(SocketSendError::Copy(SysError::BadAddress))
        );
        let mut empty = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.second_private, &mut empty),
            Err(SocketReceiveError::WouldBlock)
        );

        assert_eq!(
            send_stream_for_test(
                &pair.first_private,
                &mut PartialWriteBytes {
                    bytes: b"write",
                    limit: 2,
                },
            ),
            Ok(2)
        );
        let mut written = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.second_private, &mut written),
            Ok(2)
        );
        assert_eq!(written.0, b"wr");

        assert_eq!(
            send_stream_for_test(&pair.second_private, &mut WriteBytes(b"read")),
            Ok(4)
        );
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut FaultReadSink),
            Err(SocketReceiveError::Copy(SysError::BadAddress))
        );
        let mut prefix = PartialReadCapture {
            bytes: Vec::new(),
            limit: 2,
        };
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut prefix),
            Ok(2)
        );
        assert_eq!(prefix.bytes, b"re");
        let mut suffix = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut suffix),
            Ok(2)
        );
        assert_eq!(suffix.0, b"ad");
    }

    #[kunit]
    fn register_snapshot_and_recheck_follow_direction_predicates() {
        let pair = prepare_unix_pair().unwrap();
        let readable = Arc::new(CountingObserver(AtomicUsize::new(0)));
        let writable = Arc::new(CountingObserver(AtomicUsize::new(0)));
        let readable_route = route(&readable);
        let writable_route = route(&writable);

        assert_eq!(
            poll_unix_stream(
                &pair.first_private,
                &PollRequest::register_with_route(PollEvent::READABLE, &readable_route),
            )
            .unwrap(),
            PollRegisterResult::Subscribed(PollEvent::empty())
        );
        assert_eq!(
            poll_unix_stream(
                &pair.second_private,
                &PollRequest::register_with_route(PollEvent::WRITABLE, &writable_route),
            )
            .unwrap(),
            PollRegisterResult::Subscribed(PollEvent::WRITABLE)
        );

        assert_eq!(
            send_stream_for_test(&pair.second_private, &mut WriteBytes(b"x")),
            Ok(1)
        );
        assert_eq!(readable.notifications(), 1);
        assert_eq!(writable.notifications(), 1);
        let events = poll_unix_stream(
            &pair.first_private,
            &PollRequest::snapshot(PollEvent::READABLE),
        )
        .unwrap()
        .expect_ready("Unix readable recheck");
        assert_eq!(events, PollEvent::READABLE);

        let mut byte = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut byte),
            Ok(1)
        );
        assert_eq!(readable.notifications(), 2);
        let events = poll_unix_stream(
            &pair.first_private,
            &PollRequest::snapshot(PollEvent::READABLE),
        )
        .unwrap()
        .expect_ready("Unix empty recheck");
        assert!(events.is_empty());
    }

    #[kunit]
    fn retired_endpoint_poll_does_not_publish_route() {
        let pair = prepare_unix_pair().unwrap();
        final_release_unix_stream(&pair.first_private);
        let observer = Arc::new(CountingObserver(AtomicUsize::new(0)));
        let poll_route = route(&observer);

        let events = poll_unix_stream(
            &pair.first_private,
            &PollRequest::register_with_route(PollEvent::READABLE, &poll_route),
        )
        .unwrap()
        .expect_ready("retired Unix endpoint poll");
        assert_eq!(events, PollEvent::HANG_UP);
        assert!(
            endpoint(&pair.first_private).connection.state.lock().routes
                [EndpointSide::First.index()]
            .is_empty()
        );
    }

    #[kunit]
    fn dropping_unpublished_pair_aborts_both_endpoints() {
        let pair = prepare_unix_pair().unwrap();
        let connection = Arc::downgrade(&endpoint(&pair.first_private).connection);
        assert!(connection.upgrade().is_some());
        drop(pair);
        assert!(connection.upgrade().is_none());
    }
}
