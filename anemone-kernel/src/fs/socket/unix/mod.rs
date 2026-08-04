//! Unix connection-oriented endpoint, namespace, and family composition.

mod admission;
mod endpoint;
mod namespace;

pub(super) use endpoint::{UNIX_SEQPACKET_SOCKET_OPS, UNIX_STREAM_SOCKET_OPS};
