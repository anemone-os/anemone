mod api;
mod front;
mod icmp_raw;
mod source;
mod tcp;
mod udp;
mod unix;

use front::{
    SocketAcceptError, SocketAcceptItem, SocketAddress, SocketAddressSink, SocketBindError,
    SocketConnectError, SocketCreation, SocketDatagramSendOperation, SocketIoOps,
    SocketListenError, SocketOps, SocketOptionError, SocketOptionMutation, SocketOptionQuery,
    SocketOptionValue, SocketPairPreparation, SocketPendingError, SocketPreparation,
    SocketQueryError, SocketReadSink, SocketReceiveError, SocketReceiveFlags, SocketReceiveOutcome,
    SocketReceiveRequest, SocketReceiveSink, SocketReleaseReason, SocketSendError,
    SocketSendPayload, SocketSendRequest, SocketShutdown, SocketShutdownError,
    SocketStreamDestination, SocketType, SocketWait, SocketWriteSource, prepare_socket,
    prepare_socket_pair, retry_socket_receive, retry_socket_send, socket_file_desc_ops,
    socket_from_file, wait_for_socket_operation,
};
use icmp_raw::ICMP_RAW_SOCKET_OPS;
use tcp::TCP_SOCKET_OPS;
use udp::UDP_SOCKET_OPS;
use unix::{UNIX_SEQPACKET_SOCKET_OPS, UNIX_STREAM_SOCKET_OPS};

#[cfg(feature = "kunit")]
mod kunits {
    use super::{
        front::{AcceptedSocket, Socket},
        *,
    };

    use anemone_net_api::Ipv4Address;

    use crate::{
        fs::iomux::{PollObserver, PollRoute},
        prelude::*,
        task::files::{OpenAccessMode, OpenedFileFinalReleaseCtx},
    };

    const LISTEN_PORT: u16 = 46_211;
    const RELEASE_RACE_PORT: u16 = 46_311;
    const PEER: SocketAddress = SocketAddress::Ipv4 {
        address: Ipv4Address::LOOPBACK,
        port: LISTEN_PORT,
    };

    #[derive(Default)]
    struct AddressCapture(Option<SocketAddress>);

    impl SocketAddressSink for AddressCapture {
        fn copy_address(&mut self, address: Option<SocketAddress>) -> Result<(), SysError> {
            self.0 = address;
            Ok(())
        }
    }

    struct BytesSource<'a>(&'a [u8]);

    struct CountingObserver(AtomicUsize);

    impl CountingObserver {
        fn new() -> Self {
            Self(AtomicUsize::new(0))
        }

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

    impl SocketWriteSource for BytesSource<'_> {
        fn remaining(&self) -> usize {
            self.0.len()
        }

        fn copy_bytes(&mut self, bytes: &mut [u8]) -> Result<usize, SysError> {
            let copied = bytes.len().min(self.0.len());
            bytes[..copied].copy_from_slice(&self.0[..copied]);
            Ok(copied)
        }
    }

    struct ShortSink<'a> {
        bytes: &'a mut [u8],
        offered: usize,
    }

    impl SocketReadSink for ShortSink<'_> {
        fn remaining(&self) -> usize {
            self.offered
        }

        fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError> {
            let copied = self.bytes.len().min(bytes.len());
            self.bytes[..copied].copy_from_slice(&bytes[..copied]);
            Ok(copied)
        }
    }

    struct FaultSink(usize);

    impl SocketReadSink for FaultSink {
        fn remaining(&self) -> usize {
            self.0
        }

        fn copy_bytes(&mut self, _bytes: &[u8]) -> Result<usize, SysError> {
            Err(SysError::BadAddress)
        }
    }

    struct ReleaseDuringCopySink<'a> {
        file: &'a File,
        bytes: &'a mut [u8],
        fault: bool,
        released: bool,
    }

    impl SocketReadSink for ReleaseDuringCopySink<'_> {
        fn remaining(&self) -> usize {
            self.bytes.len()
        }

        fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError> {
            assert!(!self.released, "TCP receive sink released the file twice");
            release(self.file);
            self.released = true;
            if self.fault {
                return Err(SysError::BadAddress);
            }
            let copied = self.bytes.len().min(bytes.len());
            self.bytes[..copied].copy_from_slice(&bytes[..copied]);
            Ok(copied)
        }
    }

    fn release(file: &File) {
        (socket_file_desc_ops().final_release.unwrap())(OpenedFileFinalReleaseCtx {
            file,
            access: OpenAccessMode::ReadWrite,
            notification_suppressed: true,
        });
    }

    fn prepare_committed_tcp() -> File {
        let (file, creation) =
            prepare_socket(&TCP_SOCKET_OPS).expect("KUnit TCP Endpoint must fit");
        creation.commit();
        file
    }

    fn start_connect(socket: &Socket) {
        start_connect_to(socket, PEER.clone());
    }

    fn start_connect_to(socket: &Socket, peer: SocketAddress) {
        let Err(SocketConnectError::Started(wait)) = socket.connect(peer.clone()) else {
            panic!("first KUnit TCP connect did not return its operation wait");
        };
        assert_eq!(
            wait.poll(&PollRequest::snapshot(PollEvent::WRITABLE)),
            Ok(PollRegisterResult::Ready(PollEvent::empty()))
        );
        // No scheduling point exists between these attempts, so the owner must
        // still expose the in-progress predicate through the same source.
        assert!(matches!(
            socket.connect(peer),
            Err(SocketConnectError::InProgress(_))
        ));
    }

    fn wait_connected(socket: &Socket) {
        wait_connected_to(socket, PEER.clone());
    }

    fn wait_connected_to(socket: &Socket, peer: SocketAddress) {
        for _ in 0..20_000 {
            match socket.connect(peer.clone()) {
                Err(SocketConnectError::InProgress(_)) => yield_now(),
                Err(SocketConnectError::AlreadyConnected) => return,
                Err(SocketConnectError::ConnectionRefused) => {
                    panic!("KUnit loopback TCP connection was reset")
                },
                Err(SocketConnectError::ConnectionTimedOut) => {
                    panic!("KUnit loopback TCP connection timed out")
                },
                _ => panic!("KUnit TCP connection changed to an unexpected outcome"),
            }
        }
        panic!("KUnit loopback TCP connection did not complete");
    }

    fn accept_eventually(listener: &Socket) -> AcceptedSocket {
        for _ in 0..20_000 {
            match listener.accept() {
                Ok(accepted) => return accepted,
                Err(SocketAcceptError::WouldBlock(_)) => yield_now(),
                _ => panic!("KUnit TCP listener changed to an unexpected outcome"),
            }
        }
        panic!("KUnit TCP listener did not produce a completed child");
    }

    fn send(socket: &Socket, bytes: &[u8]) -> usize {
        let mut source = BytesSource(bytes);
        socket
            .send(SocketSendRequest::Stream {
                source: &mut source,
                destination: SocketStreamDestination::Absent,
            })
            .expect("connected KUnit TCP send must accept a prefix")
    }

    fn receive_eventually(
        socket: &Socket,
        sink: &mut dyn SocketReadSink,
    ) -> Result<SocketReceiveOutcome, SocketReceiveError> {
        receive_eventually_with_flags(socket, sink, false)
    }

    fn receive_eventually_with_flags(
        socket: &Socket,
        sink: &mut dyn SocketReadSink,
        peek: bool,
    ) -> Result<SocketReceiveOutcome, SocketReceiveError> {
        for _ in 0..20_000 {
            match socket.receive(SocketReceiveRequest::Stream {
                sink,
                flags: SocketReceiveFlags { peek },
            }) {
                Err(SocketReceiveError::WouldBlock) => yield_now(),
                result => return result,
            }
        }
        panic!("KUnit TCP receive did not observe transmitted bytes");
    }

    #[kunit]
    fn tcp_creation_rollback_and_final_release_retire_the_only_endpoint_capability() {
        let (rolled_back_file, creation) =
            prepare_socket(&TCP_SOCKET_OPS).expect("KUnit TCP Endpoint must fit");
        let rolled_back = socket_from_file(&rolled_back_file).unwrap();
        assert_eq!(rolled_back.socket_type(), SocketType::Ipv4Tcp);
        drop(creation);
        assert_eq!(
            rolled_back.copy_local_address(&mut AddressCapture::default()),
            Err(SocketQueryError::Retired)
        );

        let file = prepare_committed_tcp();
        let socket = socket_from_file(&file).unwrap();
        release(&file);
        assert_eq!(
            socket.copy_local_address(&mut AddressCapture::default()),
            Err(SocketQueryError::Retired)
        );
        let mut source = BytesSource(b"retired");
        assert_eq!(
            socket.send(SocketSendRequest::Stream {
                source: &mut source,
                destination: SocketStreamDestination::Absent,
            }),
            Err(SocketSendError::Retired)
        );
    }

    #[kunit]
    fn tcp_connect_and_accept_waits_share_production_sources_without_lost_wake() {
        let listener_file = prepare_committed_tcp();
        let listener = socket_from_file(&listener_file).unwrap();
        let address = SocketAddress::Ipv4 {
            address: Ipv4Address::LOOPBACK,
            port: LISTEN_PORT + 2,
        };
        listener.bind(address.clone()).unwrap();
        listener.listen(1).unwrap();

        let Err(SocketAcceptError::WouldBlock(accept_wait)) = listener.accept() else {
            panic!("empty KUnit listener did not return its operation wait");
        };
        let accept_observer = Arc::new(CountingObserver::new());
        assert_eq!(
            accept_wait
                .poll(&PollRequest::register_with_route(
                    PollEvent::READABLE,
                    &route(&accept_observer),
                ))
                .unwrap(),
            PollRegisterResult::Subscribed(PollEvent::empty())
        );

        let client_file = prepare_committed_tcp();
        let client = socket_from_file(&client_file).unwrap();
        let Err(SocketConnectError::Started(connect_wait)) = client.connect(address) else {
            panic!("KUnit active open did not return its operation wait");
        };
        let connect_observer = Arc::new(CountingObserver::new());
        assert_eq!(
            connect_wait
                .poll(&PollRequest::register_with_route(
                    PollEvent::WRITABLE,
                    &route(&connect_observer),
                ))
                .unwrap(),
            PollRegisterResult::Subscribed(PollEvent::empty())
        );

        let mut ready = false;
        for _ in 0..20_000 {
            let connect_ready = matches!(
                connect_wait.poll(&PollRequest::snapshot(PollEvent::WRITABLE)),
                Ok(PollRegisterResult::Ready(events)) if events.contains(PollEvent::WRITABLE)
            );
            let accept_ready = matches!(
                accept_wait.poll(&PollRequest::snapshot(PollEvent::READABLE)),
                Ok(PollRegisterResult::Ready(events)) if events.contains(PollEvent::READABLE)
            );
            if connect_ready && accept_ready {
                ready = true;
                break;
            }
            yield_now();
        }
        assert!(ready, "TCP source waits missed connection completion");
        assert!(connect_observer.notifications() > 0);
        assert!(accept_observer.notifications() > 0);
        assert!(matches!(
            client_file.poll(&PollRequest::snapshot(PollEvent::WRITABLE)),
            Ok(PollRegisterResult::Ready(events)) if events.contains(PollEvent::WRITABLE)
        ));
        assert!(matches!(
            listener_file.poll(&PollRequest::snapshot(PollEvent::READABLE)),
            Ok(PollRegisterResult::Ready(events)) if events.contains(PollEvent::READABLE)
        ));

        let accepted = match listener.accept() {
            Ok(accepted) => accepted,
            Err(_) => panic!("ready KUnit TCP listener did not return its child"),
        };
        let accepted_file = accepted.prepare_file().unwrap();
        assert_eq!(
            listener_file.poll(&PollRequest::snapshot(PollEvent::READABLE)),
            Ok(PollRegisterResult::Ready(PollEvent::empty()))
        );
        release(&accepted_file);
        release(&client_file);
        release(&listener_file);
    }

    #[kunit]
    fn tcp_general_front_closes_active_passive_stream_and_rollback_routes() {
        let listener_file = prepare_committed_tcp();
        let listener = socket_from_file(&listener_file).unwrap();
        listener
            .bind(SocketAddress::Ipv4 {
                address: Ipv4Address::LOOPBACK,
                port: LISTEN_PORT,
            })
            .unwrap();
        listener.listen(10).unwrap();
        assert_eq!(listener.is_accepting(), Ok(true));

        let first_client_file = prepare_committed_tcp();
        let first_client = socket_from_file(&first_client_file).unwrap();
        start_connect(first_client);
        wait_connected(first_client);
        let rolled_back_child = accept_eventually(listener);
        assert!(matches!(
            rolled_back_child.peer_address(),
            Some(SocketAddress::Ipv4 { address, port })
                if address == Ipv4Address::LOOPBACK && port != 0 && port != LISTEN_PORT
        ));
        // Models either peer-address copyout or file preparation failure. The
        // AcceptedSocket guard must retire rather than requeue the child.
        drop(rolled_back_child);
        release(&first_client_file);

        let client_file = prepare_committed_tcp();
        let client = socket_from_file(&client_file).unwrap();
        start_connect(client);
        wait_connected(client);
        let accepted = accept_eventually(listener);
        let accepted_file = accepted.prepare_file().unwrap();
        let accepted_socket = socket_from_file(&accepted_file).unwrap();

        let payload = b"partial-stream";
        let accepted_prefix = send(client, payload);
        assert!(accepted_prefix > 2);
        let mut prefix = [0; 2];
        let mut short = ShortSink {
            bytes: &mut prefix,
            offered: accepted_prefix,
        };
        assert_eq!(
            receive_eventually(accepted_socket, &mut short),
            Ok(SocketReceiveOutcome::byte_stream(2))
        );
        assert_eq!(prefix, payload[..2]);

        let remaining = accepted_prefix - 2;
        let mut suffix = vec![0; remaining];
        let mut suffix_sink = ShortSink {
            bytes: &mut suffix,
            offered: remaining,
        };
        assert_eq!(
            receive_eventually(accepted_socket, &mut suffix_sink),
            Ok(SocketReceiveOutcome::byte_stream(remaining))
        );
        assert_eq!(suffix, payload[2..accepted_prefix]);

        let fault_payload = b"rollback";
        let fault_prefix = send(client, fault_payload);
        assert_eq!(fault_prefix, fault_payload.len());
        assert_eq!(
            receive_eventually(accepted_socket, &mut FaultSink(fault_prefix)),
            Err(SocketReceiveError::Copy(SysError::BadAddress))
        );
        let mut recovered = vec![0; fault_prefix];
        let mut recovered_sink = ShortSink {
            bytes: &mut recovered,
            offered: fault_prefix,
        };
        assert_eq!(
            receive_eventually(accepted_socket, &mut recovered_sink),
            Ok(SocketReceiveOutcome::byte_stream(fault_prefix))
        );
        assert_eq!(recovered, fault_payload);

        assert_eq!(
            client.mutate_option(SocketOptionMutation::TcpNoDelay(true)),
            Ok(())
        );
        assert_eq!(
            client.query_option(SocketOptionQuery::TcpNoDelay),
            Ok(SocketOptionValue::Boolean(true))
        );
        assert_eq!(
            client.query_option(SocketOptionQuery::PendingError),
            Ok(SocketOptionValue::PendingError(None))
        );

        let peek_payload = b"peek-before-fin";
        assert_eq!(send(client, peek_payload), peek_payload.len());
        let mut peeked = vec![0; peek_payload.len()];
        let mut peek_sink = ShortSink {
            bytes: &mut peeked,
            offered: peek_payload.len(),
        };
        assert_eq!(
            receive_eventually_with_flags(accepted_socket, &mut peek_sink, true),
            Ok(SocketReceiveOutcome::byte_stream(peek_payload.len()))
        );
        assert_eq!(peeked, peek_payload);
        let mut consumed = vec![0; peek_payload.len()];
        let mut consume_sink = ShortSink {
            bytes: &mut consumed,
            offered: peek_payload.len(),
        };
        assert_eq!(
            receive_eventually(accepted_socket, &mut consume_sink),
            Ok(SocketReceiveOutcome::byte_stream(peek_payload.len()))
        );
        assert_eq!(consumed, peek_payload);

        assert_eq!(accepted_socket.shutdown(SocketShutdown::Write), Ok(()));
        assert_eq!(accepted_socket.shutdown(SocketShutdown::Write), Ok(()));
        let mut broken = BytesSource(b"broken");
        assert_eq!(
            accepted_socket.send(SocketSendRequest::Stream {
                source: &mut broken,
                destination: SocketStreamDestination::Absent,
            }),
            Err(SocketSendError::PeerClosed)
        );

        release(&accepted_file);
        release(&client_file);
        release(&listener_file);
    }

    #[kunit]
    fn tcp_static_final_release_defers_outstanding_consume_peek_and_fault() {
        let peer = SocketAddress::Ipv4 {
            address: Ipv4Address::LOOPBACK,
            port: RELEASE_RACE_PORT,
        };
        let listener_file = prepare_committed_tcp();
        let listener = socket_from_file(&listener_file).unwrap();
        listener.bind(peer.clone()).unwrap();
        listener.listen(10).unwrap();

        for (payload, peek, fault) in [
            (&b"consume"[..], false, false),
            (&b"peek"[..], true, false),
            (&b"fault"[..], false, true),
        ] {
            let client_file = prepare_committed_tcp();
            let client = socket_from_file(&client_file).unwrap();
            start_connect_to(client, peer.clone());
            wait_connected_to(client, peer.clone());
            let accepted_file = accept_eventually(listener).prepare_file().unwrap();
            let accepted_socket = socket_from_file(&accepted_file).unwrap();

            assert_eq!(send(client, payload), payload.len());
            let mut copied = vec![0; payload.len()];
            let mut sink = ReleaseDuringCopySink {
                file: &accepted_file,
                bytes: &mut copied,
                fault,
                released: false,
            };
            let result = receive_eventually_with_flags(accepted_socket, &mut sink, peek);
            assert!(sink.released);
            if fault {
                assert_eq!(result, Err(SocketReceiveError::Copy(SysError::BadAddress)));
            } else {
                assert_eq!(result, Ok(SocketReceiveOutcome::byte_stream(payload.len())));
                assert_eq!(copied, payload);
            }

            let mut retired = [0; 1];
            let mut retired_sink = ShortSink {
                bytes: &mut retired,
                offered: 1,
            };
            assert_eq!(
                accepted_socket.receive(SocketReceiveRequest::Stream {
                    sink: &mut retired_sink,
                    flags: SocketReceiveFlags { peek: false },
                }),
                Err(SocketReceiveError::Retired)
            );
            release(&client_file);
        }

        release(&listener_file);
    }

    #[kunit]
    fn tcp_pending_error_has_one_consumer_at_the_general_front() {
        let refused = SocketAddress::Ipv4 {
            address: Ipv4Address::LOOPBACK,
            port: LISTEN_PORT + 1,
        };
        let option_file = prepare_committed_tcp();
        let option_socket = socket_from_file(&option_file).unwrap();
        assert!(matches!(
            option_socket.connect(refused.clone()),
            Err(SocketConnectError::Started(_))
        ));
        let mut consumed = false;
        for _ in 0..20_000 {
            match option_socket.query_option(SocketOptionQuery::PendingError) {
                Ok(SocketOptionValue::PendingError(Some(
                    SocketPendingError::ConnectionRefused,
                ))) => {
                    consumed = true;
                    break;
                },
                Ok(SocketOptionValue::PendingError(None)) => yield_now(),
                outcome => panic!("TCP SO_ERROR projection changed unexpectedly: {outcome:?}"),
            }
        }
        assert!(
            consumed,
            "TCP SO_ERROR did not observe the refused connection"
        );
        assert_eq!(
            option_socket.query_option(SocketOptionQuery::PendingError),
            Ok(SocketOptionValue::PendingError(None))
        );
        assert!(matches!(
            option_socket.connect(refused.clone()),
            Err(SocketConnectError::ConnectionAborted)
        ));
        assert!(matches!(
            option_socket.connect(refused.clone()),
            Err(SocketConnectError::Started(_))
        ));
        release(&option_file);

        let operation_file = prepare_committed_tcp();
        let operation_socket = socket_from_file(&operation_file).unwrap();
        assert!(matches!(
            operation_socket.connect(refused.clone()),
            Err(SocketConnectError::Started(_))
        ));
        let mut operation_consumed = false;
        for _ in 0..20_000 {
            let mut source = BytesSource(b"x");
            match operation_socket.send(SocketSendRequest::Stream {
                source: &mut source,
                destination: SocketStreamDestination::Absent,
            }) {
                Err(SocketSendError::ConnectionRefused) => {
                    operation_consumed = true;
                    break;
                },
                Err(SocketSendError::NotConnected | SocketSendError::WouldBlock) => yield_now(),
                outcome => panic!("TCP send error projection changed unexpectedly: {outcome:?}"),
            }
        }
        assert!(
            operation_consumed,
            "ordinary TCP send did not consume the refused connection"
        );
        assert_eq!(
            operation_socket.query_option(SocketOptionQuery::PendingError),
            Ok(SocketOptionValue::PendingError(None))
        );
        assert!(matches!(
            operation_socket.connect(refused.clone()),
            Err(SocketConnectError::ConnectionAborted)
        ));
        assert!(matches!(
            operation_socket.connect(refused),
            Err(SocketConnectError::Started(_))
        ));
        release(&operation_file);
    }
}
