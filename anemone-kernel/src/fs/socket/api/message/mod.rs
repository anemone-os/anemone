//! Linux single-message Socket ABI adapters.

mod recvmsg;
mod sendmsg;

use alloc::vec::Vec;

use anemone_abi::net::linux::MsgHdr;

use crate::{
    fs::api::read_write::request::{CheckedIoVec, IoVecDirection, load_message_iovecs},
    kconfig_defs::MAX_IOVEC_COUNT,
    prelude::*,
    syscall::user_access::{UserReadPtr, user_addr},
};

use super::abi::MAX_SOCKADDR_INPUT_LEN;

pub(super) fn read_message_header(message: u64) -> Result<MsgHdr, SysError> {
    let address = user_addr(message)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    UserReadPtr::<MsgHdr>::try_new(address, &mut uspace.lock())?.read()
}

pub(super) fn message_iovecs(
    uspace: &UserSpaceHandle,
    header: MsgHdr,
    direction: IoVecDirection,
) -> Result<Vec<CheckedIoVec>, SysError> {
    let count = message_iovec_count(header)?;
    load_message_iovecs(
        uspace,
        VirtAddr::new(header.msg_iov as u64),
        count,
        direction,
    )
}

fn message_iovec_count(header: MsgHdr) -> Result<usize, SysError> {
    let count = usize::try_from(header.msg_iovlen).map_err(|_| SysError::MessageTooLong)?;
    if count > MAX_IOVEC_COUNT {
        return Err(SysError::MessageTooLong);
    }
    Ok(count)
}

pub(super) fn normalized_name_len(header: MsgHdr) -> Result<usize, SysError> {
    if header.msg_name.is_null() {
        return Ok(0);
    }
    if header.msg_namelen < 0 {
        return Err(SysError::InvalidArgument);
    }
    Ok(header.msg_namelen.min(MAX_SOCKADDR_INPUT_LEN as i32) as usize)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use core::ffi::c_void;

    use anemone_abi::net::linux::{MSG_DONTWAIT, MSG_NOSIGNAL, MSG_PEEK};
    use anemone_net_api::Ipv4Address;

    use crate::{
        fs::socket::{
            SocketAcceptError, SocketAddress, SocketConnectError, SocketReadSink,
            SocketReceiveOutcome, SocketShutdown, SocketStreamDestination, SocketWriteSource,
            TCP_SOCKET_OPS, prepare_socket, socket_file_desc_ops, socket_from_file,
        },
        task::{
            files::{OpenAccessMode, OpenedFileFinalReleaseCtx},
            sig::{SigNo, set::SigSet},
        },
    };

    static NEXT_PORT: AtomicU16 = AtomicU16::new(48_000);

    fn release(file: &File) {
        (socket_file_desc_ops().final_release.unwrap())(OpenedFileFinalReleaseCtx {
            file,
            access: OpenAccessMode::ReadWrite,
            notification_suppressed: true,
        });
    }

    struct TcpMessagePair {
        listener: File,
        client: File,
        accepted: File,
    }

    impl TcpMessagePair {
        fn new() -> Self {
            let address = SocketAddress::Ipv4 {
                address: Ipv4Address::LOOPBACK,
                port: NEXT_PORT.fetch_add(1, Ordering::Relaxed),
            };
            let (listener, listener_creation) = prepare_socket(&TCP_SOCKET_OPS).unwrap();
            listener_creation.commit();
            let listener_socket = socket_from_file(&listener).unwrap();
            assert_eq!(listener_socket.bind(address.clone()), Ok(()));
            assert_eq!(listener_socket.listen(4), Ok(()));

            let (client, client_creation) = prepare_socket(&TCP_SOCKET_OPS).unwrap();
            client_creation.commit();
            let client_socket = socket_from_file(&client).unwrap();
            assert!(matches!(
                client_socket.connect(address.clone()),
                Err(SocketConnectError::Started(_))
            ));
            let mut connected = false;
            for _ in 0..20_000 {
                match client_socket.connect(address.clone()) {
                    Err(SocketConnectError::InProgress(_)) => yield_now(),
                    Err(SocketConnectError::AlreadyConnected) => {
                        connected = true;
                        break;
                    },
                    _ => panic!("KUnit message TCP connect changed unexpectedly"),
                }
            }
            assert!(connected, "KUnit message TCP connect did not complete");

            let mut accepted = None;
            for _ in 0..20_000 {
                match listener_socket.accept() {
                    Ok(child) => {
                        accepted = Some(child.prepare_file().unwrap());
                        break;
                    },
                    Err(SocketAcceptError::WouldBlock(_)) => yield_now(),
                    _ => panic!("KUnit message TCP accept changed unexpectedly"),
                }
            }
            Self {
                listener,
                client,
                accepted: accepted.expect("KUnit message TCP accept did not complete"),
            }
        }

        fn client_socket(&self) -> &crate::fs::socket::front::Socket {
            socket_from_file(&self.client).unwrap()
        }

        fn accepted_socket(&self) -> &crate::fs::socket::front::Socket {
            socket_from_file(&self.accepted).unwrap()
        }
    }

    impl Drop for TcpMessagePair {
        fn drop(&mut self) {
            release(&self.accepted);
            release(&self.client);
            release(&self.listener);
        }
    }

    struct BytesSource<'a> {
        bytes: &'a [u8],
        copied: usize,
    }

    impl<'a> BytesSource<'a> {
        fn new(bytes: &'a [u8]) -> Self {
            Self { bytes, copied: 0 }
        }
    }

    impl SocketWriteSource for BytesSource<'_> {
        fn remaining(&self) -> usize {
            self.bytes.len() - self.copied
        }

        fn copy_bytes(&mut self, output: &mut [u8]) -> Result<usize, SysError> {
            let copied = output.len().min(self.remaining());
            output[..copied].copy_from_slice(&self.bytes[self.copied..self.copied + copied]);
            self.copied += copied;
            Ok(copied)
        }
    }

    struct FaultSource(usize);

    impl SocketWriteSource for FaultSource {
        fn remaining(&self) -> usize {
            self.0
        }

        fn copy_bytes(&mut self, _output: &mut [u8]) -> Result<usize, SysError> {
            Err(SysError::BadAddress)
        }
    }

    struct CaptureSink {
        bytes: Vec<u8>,
        offered: usize,
        fault: bool,
    }

    impl CaptureSink {
        fn new(offered: usize) -> Self {
            Self {
                bytes: Vec::new(),
                offered,
                fault: false,
            }
        }

        fn fault(offered: usize) -> Self {
            Self {
                bytes: Vec::new(),
                offered,
                fault: true,
            }
        }
    }

    impl SocketReadSink for CaptureSink {
        fn remaining(&self) -> usize {
            self.offered
        }

        fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError> {
            if self.fault {
                return Err(SysError::BadAddress);
            }
            let copied = bytes.len().min(self.offered);
            self.bytes.extend_from_slice(&bytes[..copied]);
            Ok(copied)
        }
    }

    fn send(
        pair: &TcpMessagePair,
        source: &mut dyn SocketWriteSource,
        destination: SocketStreamDestination,
        flags: i32,
    ) -> Result<u64, SysError> {
        sendmsg::send_stream_message(
            &get_current_task(),
            &pair.client,
            pair.client_socket(),
            source,
            destination,
            flags,
            true,
        )
    }

    fn receive(
        pair: &TcpMessagePair,
        sink: &mut dyn SocketReadSink,
        flags: i32,
    ) -> Result<SocketReceiveOutcome, SysError> {
        for _ in 0..20_000 {
            match recvmsg::receive_stream_message(
                &get_current_task(),
                &pair.accepted,
                pair.accepted_socket(),
                sink,
                flags | MSG_DONTWAIT,
                true,
            ) {
                Err(SysError::Again) => yield_now(),
                result => return result,
            }
        }
        panic!("KUnit stream message receive did not observe owner progress")
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum OutputField {
        NameLength,
        Flags,
        ControlLength,
    }

    struct OutputCapture {
        attempted: Vec<OutputField>,
        fault: Option<OutputField>,
    }

    impl OutputCapture {
        fn new(fault: Option<OutputField>) -> Self {
            Self {
                attempted: Vec::new(),
                fault,
            }
        }

        fn write(&mut self, field: OutputField) -> Result<(), SysError> {
            self.attempted.push(field);
            if self.fault == Some(field) {
                Err(SysError::BadAddress)
            } else {
                Ok(())
            }
        }
    }

    impl recvmsg::StreamMessageOutput for OutputCapture {
        fn write_name_len_zero(&mut self) -> Result<(), SysError> {
            self.write(OutputField::NameLength)
        }

        fn write_flags_zero(&mut self) -> Result<(), SysError> {
            self.write(OutputField::Flags)
        }

        fn write_control_len_zero(&mut self) -> Result<(), SysError> {
            self.write(OutputField::ControlLength)
        }
    }

    #[kunit]
    fn message_header_count_name_and_segment_bounds_are_checked_before_operation() {
        let mut header = MsgHdr {
            msg_namelen: -1,
            ..MsgHdr::default()
        };
        assert_eq!(normalized_name_len(header), Ok(0));
        header.msg_name = 1usize as *mut c_void;
        assert_eq!(normalized_name_len(header), Err(SysError::InvalidArgument));

        header.msg_iovlen = (MAX_IOVEC_COUNT + 1) as u64;
        assert_eq!(message_iovec_count(header), Err(SysError::MessageTooLong));
        header.msg_iovlen = MAX_IOVEC_COUNT as u64;
        assert_eq!(message_iovec_count(header), Ok(MAX_IOVEC_COUNT));

        let iovecs = [
            CheckedIoVec {
                base: VirtAddr::new(0),
                len: usize::MAX,
            },
            CheckedIoVec {
                base: VirtAddr::new(0),
                len: 1,
            },
        ];
        assert!(matches!(
            sendmsg::message_segments(&iovecs),
            Err(SysError::MessageTooLong)
        ));
        assert_eq!(
            sendmsg::stream_destination(true),
            SocketStreamDestination::Present
        );
        assert_eq!(sendmsg::validate_send_control(0), Ok(()));
        assert_eq!(
            sendmsg::validate_send_control(1),
            Err(SysError::NotSupported)
        );
    }

    #[kunit]
    fn tcp_stream_message_name_vector_short_fault_and_peek_share_the_descriptor_path() {
        let pair = TcpMessagePair::new();
        let mut vector = BytesSource::new(b"vector");
        assert_eq!(
            send(&pair, &mut vector, SocketStreamDestination::Present, 0,),
            Ok(6)
        );

        let mut fault = CaptureSink::fault(6);
        assert_eq!(receive(&pair, &mut fault, 0), Err(SysError::BadAddress));
        let mut peek = CaptureSink::new(6);
        assert_eq!(
            receive(&pair, &mut peek, MSG_PEEK),
            Ok(SocketReceiveOutcome::byte_stream(6))
        );
        assert_eq!(peek.bytes, b"vector");
        let mut short = CaptureSink::new(2);
        assert_eq!(
            receive(&pair, &mut short, 0),
            Ok(SocketReceiveOutcome::byte_stream(2))
        );
        assert_eq!(short.bytes, b"ve");
        let mut tail = CaptureSink::new(4);
        assert_eq!(
            receive(&pair, &mut tail, 0),
            Ok(SocketReceiveOutcome::byte_stream(4))
        );
        assert_eq!(tail.bytes, b"ctor");

        let mut source_fault = FaultSource(4);
        assert_eq!(
            send(&pair, &mut source_fault, SocketStreamDestination::Absent, 0,),
            Err(SysError::BadAddress)
        );
        let mut empty = CaptureSink::new(1);
        assert_eq!(
            recvmsg::receive_stream_message(
                &get_current_task(),
                &pair.accepted,
                pair.accepted_socket(),
                &mut empty,
                MSG_DONTWAIT,
                true,
            ),
            Err(SysError::Again)
        );
    }

    #[kunit]
    fn tcp_stream_message_output_order_and_fault_follow_payload_consumption() {
        let pair = TcpMessagePair::new();
        let mut source = BytesSource::new(b"ordered");
        assert_eq!(
            send(&pair, &mut source, SocketStreamDestination::Absent, 0,),
            Ok(7)
        );
        let mut sink = CaptureSink::new(7);
        assert_eq!(
            receive(&pair, &mut sink, 0),
            Ok(SocketReceiveOutcome::byte_stream(7))
        );
        assert_eq!(sink.bytes, b"ordered");

        let mut output = OutputCapture::new(Some(OutputField::Flags));
        assert_eq!(
            recvmsg::write_stream_message_output(&mut output, true),
            Err(SysError::BadAddress)
        );
        assert_eq!(
            output.attempted,
            vec![OutputField::NameLength, OutputField::Flags]
        );
        let mut empty = CaptureSink::new(1);
        assert_eq!(
            recvmsg::receive_stream_message(
                &get_current_task(),
                &pair.accepted,
                pair.accepted_socket(),
                &mut empty,
                MSG_DONTWAIT,
                true,
            ),
            Err(SysError::Again)
        );

        let mut complete = OutputCapture::new(None);
        recvmsg::write_stream_message_output(&mut complete, true).unwrap();
        assert_eq!(
            complete.attempted,
            vec![
                OutputField::NameLength,
                OutputField::Flags,
                OutputField::ControlLength
            ]
        );
    }

    #[kunit]
    fn tcp_stream_message_zero_capacity_sigpipe_and_no_signal_use_shared_retry() {
        let pair = TcpMessagePair::new();
        let mut zero = BytesSource::new(&[]);
        assert_eq!(
            send(&pair, &mut zero, SocketStreamDestination::Absent, 0,),
            Ok(0)
        );

        let bytes = vec![0x6b; crate::kconfig_defs::NET_TCP_TX_BUFFER_BYTES + 64];
        let mut large = BytesSource::new(&bytes);
        let sent = send(&pair, &mut large, SocketStreamDestination::Absent, 0).unwrap() as usize;
        assert!(sent > 0 && sent < bytes.len());

        assert_eq!(pair.client_socket().shutdown(SocketShutdown::Write), Ok(()));
        let task = get_current_task();
        let sigpipe = SigSet::new_with_signos(&[SigNo::SIGPIPE]);
        assert!(!task.pending_signal_set().get(SigNo::SIGPIPE));
        let mut zero = BytesSource::new(&[]);
        assert_eq!(
            send(&pair, &mut zero, SocketStreamDestination::Absent, 0,),
            Err(SysError::BrokenPipe)
        );
        assert!(task.pending_signal_set().get(SigNo::SIGPIPE));
        task.get_thread_group().flush_specific_signals(sigpipe);

        let mut zero = BytesSource::new(&[]);
        assert_eq!(
            send(
                &pair,
                &mut zero,
                SocketStreamDestination::Absent,
                MSG_NOSIGNAL,
            ),
            Err(SysError::BrokenPipe)
        );
        assert!(!task.pending_signal_set().get(SigNo::SIGPIPE));
    }
}
