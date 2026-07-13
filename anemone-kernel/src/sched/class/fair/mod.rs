//! Stable fair scheduler class identity.

use crate::prelude::*;

#[cfg(feature = "kunit")]
use super::entity::{SchedClassPrv, SchedEntity};

mod stride;

pub(super) use stride::{Stride as Fair, StrideEntity as FairEntity};

/// Linux `sched_prio_to_weight[]` for the complete `Nice[-20, 19]` domain.
const NICE_WEIGHTS: [u32; Nice::WIDTH] = [
    88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14949, 11916, 9548, 7620, 6100, 4904,
    3906, 3121, 2501, 1991, 1586, 1277, 1024, 820, 655, 526, 423, 335, 272, 215, 172, 137, 110, 87,
    70, 56, 45, 36, 29, 23, 18, 15,
];

fn nice_weight(nice: Nice) -> u32 {
    let weight = NICE_WEIGHTS[nice.table_index()];
    assert!(weight > 0, "Fair nice weight must be positive");
    weight
}

pub(super) fn new_fresh_entity() -> FairEntity {
    FairEntity::new_fresh()
}

/// Construct a Fair integration-test entity through the same owner factory as
/// the production default facade, without depending on the global selector.
#[cfg(feature = "kunit")]
pub(super) fn new_test_entity() -> SchedEntity {
    SchedEntity::new(SchedClassPrv::Fair(new_fresh_entity()))
}
