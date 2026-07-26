use crate::prelude::*;

static_assert!(
    NET_PUMP_INGRESS_BUDGET_FRAMES > 0,
    "NET_PUMP_INGRESS_BUDGET_FRAMES must be non-zero"
);
static_assert!(
    NET_PUMP_EGRESS_BUDGET_STEPS > 0,
    "NET_PUMP_EGRESS_BUDGET_STEPS must be non-zero"
);
static_assert!(
    NET_WORKER_REPOLL_ROUNDS > 0,
    "NET_WORKER_REPOLL_ROUNDS must be non-zero"
);
