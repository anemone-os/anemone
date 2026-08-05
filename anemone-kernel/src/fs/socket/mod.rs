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
    SocketOptionValue, SocketPairPreparation, SocketPreparation, SocketQueryError, SocketReadSink,
    SocketReceiveError, SocketReceiveFlags, SocketReceiveOutcome, SocketReceiveRequest,
    SocketReceiveSink, SocketSendError, SocketSendPayload, SocketSendRequest, SocketShutdown,
    SocketShutdownError, SocketStreamDestination, SocketType, SocketWait, SocketWriteSource,
    prepare_socket, prepare_socket_pair, retry_socket_receive, retry_socket_send,
    socket_file_desc_ops, socket_from_file, wait_for_socket_operation,
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
        prelude::*,
        task::files::{OpenAccessMode, OpenedFileFinalReleaseCtx},
    };

    const LISTEN_PORT: u16 = 46_211;
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
        assert!(matches!(
            socket.connect(PEER.clone()),
            Err(SocketConnectError::Started)
        ));
        // No scheduling point exists between these attempts, so the owner must
        // still expose the distinct in-progress observation.
        assert!(matches!(
            socket.connect(PEER.clone()),
            Err(SocketConnectError::InProgress)
        ));
    }

    fn wait_connected(socket: &Socket) {
        for _ in 0..20_000 {
            match socket.connect(PEER.clone()) {
                Err(SocketConnectError::InProgress) => yield_now(),
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
        for _ in 0..20_000 {
            match socket.receive(SocketReceiveRequest::Stream {
                sink,
                flags: SocketReceiveFlags { peek: false },
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

        release(&accepted_file);
        release(&client_file);
        release(&listener_file);
    }
}
