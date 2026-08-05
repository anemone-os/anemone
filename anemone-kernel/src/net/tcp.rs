//! Construction-time policy for the Stack-private TCP owner.

use anemone_smoltcp_stack::TcpPolicy;

use crate::{kconfig_defs::*, prelude::*};

pub(in crate::net) const TCP_POLICY: TcpPolicy = TcpPolicy::new(
    NET_TCP_ENDPOINT_CAPACITY,
    NET_TCP_ENGINE_TIMER_CAPACITY,
    NET_TCP_LISTENER_COMPLETED_CAPACITY,
    NET_TCP_RX_BUFFER_BYTES,
    NET_TCP_TX_BUFFER_BYTES,
    NET_TCP_DEFERRED_RECLAIM_CAPACITY,
);

const fn engine_storage_fits() -> bool {
    let Some(bytes_per_engine) = NET_TCP_RX_BUFFER_BYTES.checked_add(NET_TCP_TX_BUFFER_BYTES)
    else {
        return false;
    };
    let Some(total) = NET_TCP_ENGINE_TIMER_CAPACITY.checked_mul(bytes_per_engine) else {
        return false;
    };
    total <= isize::MAX as usize
}

static_assert!(
    NET_TCP_ENDPOINT_CAPACITY > 1,
    "net_tcp_endpoint_capacity must fit one listener and one accepted endpoint"
);
static_assert!(
    NET_TCP_ENGINE_TIMER_CAPACITY > 0,
    "net_tcp_engine_timer_capacity must be nonzero"
);
static_assert!(
    NET_TCP_LISTENER_COMPLETED_CAPACITY >= 10,
    "net_tcp_listener_completed_capacity must retain at least ten completed children"
);
static_assert!(
    NET_TCP_ENGINE_TIMER_CAPACITY > NET_TCP_LISTENER_COMPLETED_CAPACITY,
    "net_tcp_engine_timer_capacity must fit one listener and an accepted child"
);
static_assert!(
    NET_TCP_RX_BUFFER_BYTES > 0 && NET_TCP_TX_BUFFER_BYTES > 0,
    "TCP engine buffers must be nonzero"
);
static_assert!(
    NET_TCP_DEFERRED_RECLAIM_CAPACITY > 0
        && NET_TCP_DEFERRED_RECLAIM_CAPACITY <= NET_TCP_ENGINE_TIMER_CAPACITY,
    "TCP deferred reclaim capacity must fit the engine bound"
);
static_assert!(
    engine_storage_fits(),
    "configured TCP engine storage exceeds the contiguous byte-storage bound"
);
