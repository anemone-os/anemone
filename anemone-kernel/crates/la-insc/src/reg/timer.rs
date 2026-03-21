//! Timer register wrappers
use crate::{impl_bits64, impl_const_u64_converter};

/// TCFG register wrapper
pub struct Tcfg(u64);
impl Tcfg {
    impl_bits64!(bool, periodic, 1);
    impl_bits64!(bool, enabled, 0);
    impl_bits64!(number, initval, u64, 2, 63);
    impl_const_u64_converter!();

    /// Create a TCFG value.
    pub const fn new(initval: u64, periodic: bool, enabled: bool) -> Self {
        let mut config = Self(0);
        config.set_periodic(periodic);
        config.set_enabled(enabled);
        config.set_initval(initval);
        config
    }
}
