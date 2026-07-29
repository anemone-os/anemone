mod datagram;
mod endpoint;
mod namespace;

// Preserve the pre-split crate-private paths, including paths used only by
// conditional consumers.
pub(crate) use anemone_net_api::udp::UdpEndpointId as EndpointId;
#[allow(unused_imports)]
pub(crate) use datagram::ReceivedDatagram;
#[allow(unused_imports)]
pub(crate) use endpoint::EngineResource;
pub(crate) use endpoint::{Endpoint, SendError};
pub(crate) use namespace::UdpEndpoints;
