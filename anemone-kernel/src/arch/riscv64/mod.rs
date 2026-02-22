// Note:
// Some logic or data structures may be sharable between RiscV32 and RiscV64,
// but for simplicity and clarity, we name them all with "RiscV64" prefix for
// now. We can always refactor them later when RiscV32 support is added.

pub(super) mod cpu;
pub(super) mod exception;
pub(super) mod paging;
pub(super) mod power;
pub(super) mod time;

mod bootstrap;
