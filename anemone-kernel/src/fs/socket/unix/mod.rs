//! Unix stream endpoint, namespace, and family composition.

mod admission;
mod endpoint;
mod namespace;

pub(super) use endpoint::UNIX_STREAM_SOCKET_OPS;
