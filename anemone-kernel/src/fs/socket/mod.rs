mod api;
mod front;
mod udp;
mod unix;

use front::{
    SocketAcceptError, SocketAcceptItem, SocketAddress, SocketAddressSink, SocketBindError,
    SocketConnectError, SocketCreation, SocketListenError, SocketOps, SocketPairPreparation,
    SocketPreparation, SocketQueryError, SocketReceiveError, SocketReceiveFlags,
    SocketReceiveRequest, SocketReceiveSink, SocketSendError, SocketSendPayload, SocketSendRequest,
    SocketShutdown, SocketShutdownError, SocketStreamDestination, SocketStreamReadSink,
    SocketStreamWriteSource, SocketType, SocketWait, prepare_socket, prepare_socket_pair,
    send_sigpipe, socket_file_desc_ops, socket_from_file,
};
use udp::UDP_SOCKET_OPS;
use unix::UNIX_STREAM_SOCKET_OPS;
