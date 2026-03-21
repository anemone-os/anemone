//! CRMD register wrapper

use crate::{impl_bits64, impl_const_u64_converter, impl_rw};

/// CRMD register wrapper
/// 
/// Create from zero is not allowed, so the `new` function is not provided.
pub struct Crmd(u64);
impl Crmd {
    impl_bits64!(bool, ie, 2);
    
    impl_const_u64_converter!();
}

impl_rw!(crmd, ie, bool);