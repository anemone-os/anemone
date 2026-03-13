use crate::prelude::*;

pub trait PlatformDriver: Driver {
    /// This method is put into trait instead of being a field in
    /// `PlatformDriverBase` cz the match table is usually a static array,
    /// so no need to allocate memory for it in each driver instance.
    fn match_table(&self) -> &[&str];
}
