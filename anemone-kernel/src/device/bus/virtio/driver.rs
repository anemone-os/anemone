use crate::prelude::*;

pub trait VirtIODriver: Driver {
    /// Device Id table for matching with VirtIO devices.
    ///
    /// Currently we don't consider vendor-specific devices, so the id table is
    /// just a list of device types.
    fn id_table(&self) -> &'static [usize];
}
