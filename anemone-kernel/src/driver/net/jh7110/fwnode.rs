use crate::{
    device::{bus::platform::PlatformDevice, discovery::fwnode::FwNode, resource::Resource},
    prelude::*,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GmacFwConfig {
    pub(super) mmio: (PhysAddr, usize),
    pub(super) mac: [u8; 6],
}

impl GmacFwConfig {
    pub(super) fn parse(device: &PlatformDevice) -> Result<Self, SysError> {
        let fwnode = device.fwnode().ok_or(SysError::MissingFwNode)?;
        Self::parse_parts(fwnode.as_ref(), device.resources())
    }

    fn parse_parts(fwnode: &dyn FwNode, resources: &[Resource]) -> Result<Self, SysError> {
        let mmio = single_mmio_resource(resources).ok_or(SysError::MissingResource)?;
        if fwnode.prop_read_str("phy-mode").as_deref() != Some("rgmii-id") {
            return Err(SysError::DriverIncompatible);
        }
        let mac = parse_mac(fwnode).ok_or(SysError::DriverIncompatible)?;
        Ok(Self { mmio, mac })
    }
}

fn single_mmio_resource(resources: &[Resource]) -> Option<(PhysAddr, usize)> {
    let mut selected = None;
    for resource in resources {
        let Resource::Mmio { base, len } = resource;
        if selected.is_some() || *len == 0 {
            return None;
        }
        selected = Some((*base, *len));
    }
    selected
}

fn parse_mac(fwnode: &dyn FwNode) -> Option<[u8; 6]> {
    let mac = fwnode.prop_read_raw("local-mac-address")?.try_into().ok()?;
    valid_mac(mac).then_some(mac)
}

fn valid_mac(mac: [u8; 6]) -> bool {
    mac.iter().any(|byte| *byte != 0) && mac[0] & 1 == 0
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::device::discovery::fwnode::{FwNode, StdoutConfig};

    #[derive(Debug)]
    struct FakeFwNode {
        phy_mode: Option<&'static str>,
        local_mac_address: Option<Vec<u8>>,
    }

    impl FwNode for FakeFwNode {
        fn equals(&self, _other: &dyn FwNode) -> bool {
            false
        }

        fn prop_read_u32(&self, _prop_name: &str) -> Option<u32> {
            None
        }
        fn prop_read_u64(&self, _prop_name: &str) -> Option<u64> {
            None
        }

        fn prop_read_str(&self, prop_name: &str) -> Option<String> {
            (prop_name == "phy-mode")
                .then(|| self.phy_mode.map(ToString::to_string))
                .flatten()
        }

        fn prop_read_present(&self, prop_name: &str) -> bool {
            self.prop_read_raw(prop_name).is_some()
        }

        fn prop_read_raw(&self, prop_name: &str) -> Option<&[u8]> {
            match prop_name {
                "local-mac-address" => self
                    .local_mac_address
                    .as_ref()
                    .map(|value| value.as_slice()),
                _ => None,
            }
        }

        fn interrupt_parent(&self) -> Option<Arc<dyn FwNode>> {
            None
        }
        fn interrupt_info(&self) -> Option<&[u8]> {
            None
        }
        fn stdout_config(&self) -> Option<StdoutConfig<'_>> {
            None
        }
    }

    #[kunit]
    fn local_mac_source_is_required_and_rejects_invalid_values() {
        let local = [0x02, 0, 0, 0, 0, 2];
        let node = FakeFwNode {
            phy_mode: Some("rgmii-id"),
            local_mac_address: Some(local.to_vec()),
        };
        let config = GmacFwConfig::parse_parts(
            &node,
            &[Resource::mmio(PhysAddr::new(0x1603_0000), 0x10000)],
        )
        .unwrap();
        assert_eq!(config.mac, local);

        let invalid = [0xff; 6];
        let node = FakeFwNode {
            phy_mode: Some("rgmii-id"),
            local_mac_address: Some(invalid.to_vec()),
        };
        assert!(
            GmacFwConfig::parse_parts(
                &node,
                &[Resource::mmio(PhysAddr::new(0x1604_0000), 0x10000)],
            )
            .is_err()
        );
    }

    #[kunit]
    fn missing_zero_and_wrong_length_local_mac_fail_closed() {
        for local_mac_address in [None, Some(vec![0; 6]), Some(vec![0x02; 5])] {
            let node = FakeFwNode {
                phy_mode: Some("rgmii-id"),
                local_mac_address,
            };
            assert!(
                GmacFwConfig::parse_parts(
                    &node,
                    &[Resource::mmio(PhysAddr::new(0x1603_0000), 0x10000)]
                )
                .is_err()
            );
        }
    }

    #[kunit]
    fn independent_candidates_do_not_share_probe_failure() {
        let mac = [0x02, 0, 0, 0, 0, 1];
        let failed = FakeFwNode {
            phy_mode: Some("mii"),
            local_mac_address: None,
        };
        let middle = FakeFwNode {
            phy_mode: Some("mii"),
            local_mac_address: Some(vec![0x02, 0, 0, 0, 0, 2]),
        };
        let ready = FakeFwNode {
            phy_mode: Some("rgmii-id"),
            local_mac_address: Some(mac.to_vec()),
        };
        let tail = FakeFwNode {
            phy_mode: Some("rgmii-id"),
            local_mac_address: Some(vec![0x02, 0, 0, 0, 0, 3]),
        };
        assert!(
            GmacFwConfig::parse_parts(
                &failed,
                &[Resource::mmio(PhysAddr::new(0x1603_0000), 0x10000)]
            )
            .is_err()
        );
        assert!(
            GmacFwConfig::parse_parts(
                &middle,
                &[Resource::mmio(PhysAddr::new(0x1604_0000), 0x10000)]
            )
            .is_err()
        );
        assert!(
            GmacFwConfig::parse_parts(
                &ready,
                &[Resource::mmio(PhysAddr::new(0x1604_0000), 0x10000)]
            )
            .is_ok()
        );
        assert!(
            GmacFwConfig::parse_parts(
                &tail,
                &[Resource::mmio(PhysAddr::new(0x1605_0000), 0x10000)]
            )
            .is_ok()
        );
    }
}
