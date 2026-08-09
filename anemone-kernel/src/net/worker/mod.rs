//! Bounded network-worker scheduling and external-provider progression.

mod control;
mod external;
mod local;

use anemone_net_api::Instant as NetworkInstant;
use anemone_smoltcp_stack::PumpBudget;

use crate::prelude::*;

pub(super) use control::PumpControl;
use control::{deadline_due, schedule_deadline};
pub(super) use external::{AttachFailure, PreparedPath, prepare};
pub(super) use local::{PreparedLocalPath, prepare_local};

static_assert!(
    NET_PUMP_INGRESS_BUDGET_FRAMES > 0,
    "NET_PUMP_INGRESS_BUDGET_FRAMES must be non-zero"
);
static_assert!(
    NET_LOCAL_LINK_PACKET_CAPACITY > 0,
    "NET_LOCAL_LINK_PACKET_CAPACITY must be non-zero"
);
static_assert!(
    NET_LOCAL_LINK_MTU_BYTES >= 28,
    "NET_LOCAL_LINK_MTU_BYTES must fit IPv4 and UDP fixed headers"
);
static_assert!(
    NET_LOCAL_LINK_MTU_BYTES <= u32::MAX as usize,
    "NET_LOCAL_LINK_MTU_BYTES must fit Linux IFLA_MTU"
);
static_assert!(
    NET_PUMP_EGRESS_BUDGET_STEPS > 0,
    "NET_PUMP_EGRESS_BUDGET_STEPS must be non-zero"
);
static_assert!(
    NET_WORKER_REPOLL_ROUNDS > 0,
    "NET_WORKER_REPOLL_ROUNDS must be non-zero"
);

const PUMP_BUDGET: PumpBudget =
    PumpBudget::new(NET_PUMP_INGRESS_BUDGET_FRAMES, NET_PUMP_EGRESS_BUDGET_STEPS);

pub(in crate::net) fn network_now() -> NetworkInstant {
    let micros = crate::time::MonotonicInstant::now()
        .to_duration()
        .as_micros();
    let micros = i64::try_from(micros).unwrap_or(i64::MAX);
    NetworkInstant::from_micros(micros)
}
