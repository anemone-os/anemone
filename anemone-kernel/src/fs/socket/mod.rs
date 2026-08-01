mod api;
mod front;
mod udp;

use front::{
    SocketAddress, SocketAddressSink, SocketBindError, SocketCreation, SocketOps,
    SocketPreparation, SocketQueryError, SocketReceiveError, SocketReceiveSink, SocketSendError,
    SocketSendPayload, SocketType, prepare_socket, socket_file_desc_ops, socket_from_file,
};
use udp::UDP_SOCKET_OPS;
