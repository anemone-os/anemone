//! Synopsys legacy DWMAC1000 concrete backend registration.

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        kobject::{KObjIdent, KObjectBase, KObjectOps},
    },
    prelude::*,
};

use super::compatible_matches;

const COMPATIBLES: [&str; 2] = ["snps,dwmac-3.70a", "snps,arc-dwmac-3.70a"];

#[derive(Debug, KObject, Driver)]
struct Driver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for Driver {}

impl DriverOps for Driver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let pdev = device
            .as_platform_device()
            .ok_or(SysError::DriverIncompatible)?;
        if !compatible_matches(pdev, &COMPATIBLES) {
            return Err(SysError::DriverIncompatible);
        }
        // Gate 1 registers the owner and match table. Gate 2 owns the first
        // hardware side effect and replaces this fail-closed result.
        kinfoln!(
            "dwmac1000 {}: backend registered but not enabled until Gate 2",
            device.name()
        );
        Err(SysError::NotSupported)
    }

    fn shutdown(&self, _device: &dyn Device) {}

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }
}

impl PlatformDriver for Driver {
    fn match_table(&self) -> &[&str] {
        &COMPATIBLES
    }
}

#[initcall(driver)]
fn init() {
    platform::register_driver(Arc::new(Driver {
        kobj_base: KObjectBase::new(KObjIdent::try_from("dwmac1000").unwrap()),
        drv_base: DriverBase::new(),
    }));
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn compatible_tables_keep_driver_ownership_separate() {
        assert!(COMPATIBLES.contains(&"snps,dwmac-3.70a"));
        assert!(!COMPATIBLES.contains(&"starfive,jh7110-dwmac"));
    }
}
