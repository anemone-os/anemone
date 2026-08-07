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
    SocketIpv4ExtendedError, SocketListenError, SocketOps, SocketOptionError, SocketOptionMutation,
    SocketOptionQuery, SocketOptionValue, SocketPairPreparation, SocketPendingError,
    SocketPreparation, SocketQueryError, SocketReadSink, SocketReceiveError, SocketReceiveFlags,
    SocketReceiveOutcome, SocketReceiveRequest, SocketReceiveSink, SocketReleaseReason,
    SocketSendError, SocketSendPayload, SocketSendRequest, SocketShutdown, SocketShutdownError,
    SocketStreamDestination, SocketType, SocketWait, SocketWriteSource, pending_error_to_sys_error,
    prepare_socket, prepare_socket_pair, retry_socket_receive, retry_socket_send,
    socket_file_desc_ops, socket_from_file, wait_for_socket_operation,
};
use icmp_raw::ICMP_RAW_SOCKET_OPS;
use tcp::TCP_SOCKET_OPS;
use udp::UDP_SOCKET_OPS;
use unix::{UNIX_SEQPACKET_SOCKET_OPS, UNIX_STREAM_SOCKET_OPS};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use crate::{
        prelude::*,
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
}
