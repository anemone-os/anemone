//! smoltcp `phy::Device` adapter over [`NetDev`](crate::device::net::NetDev).

use alloc::{sync::Arc, vec, vec::Vec};

use smoltcp::{
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    time::Instant as SmolInstant,
};

use crate::{
    device::net::{NetDev, PhyCapabilities, PhyMedium},
    prelude::*,
};

pub(crate) fn smoltcp_device_capabilities(phy: PhyCapabilities) -> DeviceCapabilities {
    let mut caps = DeviceCapabilities::default();
    caps.max_transmission_unit = phy.max_transmission_unit;
    caps.medium = match phy.medium {
        PhyMedium::Ethernet => Medium::Ethernet,
    };
    caps
}

pub(crate) struct NetDeviceAdapter {
    pub(crate) netdev: Arc<dyn NetDev>,
}

impl NetDeviceAdapter {
    pub(crate) fn netdev(&self) -> Arc<dyn NetDev> {
        self.netdev.clone()
    }
}

pub(crate) struct NetRxToken {
    data: Vec<u8>,
}

pub(crate) struct NetTxToken {
    netdev: Arc<dyn NetDev>,
}

impl RxToken for NetRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.data)
    }
}

impl TxToken for NetTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buf = vec![0u8; len];
        let result = f(&mut buf);
        self.netdev.with_phy_mut(&mut |phy| {
            if phy.send_raw(&buf).is_err() {
                kerrln!("net: tx failed on send_raw");
            }
        });
        result
    }
}

impl Device for NetDeviceAdapter {
    type RxToken<'a> = NetRxToken;
    type TxToken<'a> = NetTxToken;

    fn receive(
        &mut self,
        _timestamp: SmolInstant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let mut frame: Option<Vec<u8>> = None;
        self.netdev.with_phy_mut(&mut |phy| {
            frame = phy.try_recv_frame();
        });
        let data = frame?;
        Some((
            NetRxToken { data },
            NetTxToken {
                netdev: self.netdev.clone(),
            },
        ))
    }

    fn transmit(&mut self, _timestamp: SmolInstant) -> Option<Self::TxToken<'_>> {
        let mut can = false;
        self.netdev.with_phy_mut(&mut |phy| {
            can = phy.can_send();
        });
        if !can {
            return None;
        }
        Some(NetTxToken {
            netdev: self.netdev.clone(),
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut phy_caps = PhyCapabilities {
            max_transmission_unit: 0,
            medium: PhyMedium::Ethernet,
        };
        self.netdev.with_phy_mut(&mut |phy| {
            phy_caps = phy.capabilities();
        });
        smoltcp_device_capabilities(phy_caps)
    }
}
