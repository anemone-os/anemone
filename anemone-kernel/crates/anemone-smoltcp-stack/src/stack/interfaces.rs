//! External-interface mapping owned by the protocol Stack.

use alloc::vec::Vec;

use anemone_net_api::{EthernetAddress, FrameProvider, Instant, InterfaceId};
use smoltcp::{
    iface::{Config, Interface, SocketSet},
    wire::{EthernetAddress as SmoltcpEthernetAddress, HardwareAddress},
};

use crate::adapter::FrameDevice;

use super::{PumpError, Stack};

#[derive(Clone, Copy)]
pub(crate) enum PumpOrder {
    IngressFirst,
    EgressFirst,
}

impl PumpOrder {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::IngressFirst => Self::EgressFirst,
            Self::EgressFirst => Self::IngressFirst,
        }
    }
}

pub(crate) struct InterfaceEntry {
    pub(crate) id: InterfaceId,
    // Stable attach-time snapshot required by smoltcp. The provider remains
    // the truth source; every pump asserts that this snapshot is not stale.
    pub(crate) frame_capacity: usize,
    pub(crate) interface: Interface,
    pub(crate) sockets: SocketSet<'static>,
    // This owner-local cursor chooses only the next software admission order.
    // It is not queue, link, resource, or deadline truth and never bypasses
    // either direction's finite PumpBudget.
    pub(crate) next_pump_order: PumpOrder,
}

impl Stack {
    pub fn add_interface<P: FrameProvider>(
        &mut self,
        provider: &mut P,
        ethernet_address: EthernetAddress,
        now: Instant,
    ) -> InterfaceId {
        let raw_id = self.next_interface_id;
        self.next_interface_id = raw_id
            .checked_add(1)
            .expect("InterfaceId namespace exhausted");
        let id = InterfaceId::from_index(raw_id);
        let frame_capacity = provider.capabilities().max_frame_len;
        let mut device = FrameDevice::new(provider);
        let hardware_address = HardwareAddress::Ethernet(SmoltcpEthernetAddress::from_bytes(
            &ethernet_address.octets(),
        ));
        let interface = Interface::new(
            Config::new(hardware_address),
            &mut device,
            crate::adapter::to_smoltcp_instant(now),
        );

        let mut sockets = SocketSet::new(Vec::new());
        self.udp.add_interface(id, &mut sockets);
        self.interfaces.push(InterfaceEntry {
            id,
            frame_capacity,
            interface,
            sockets,
            next_pump_order: PumpOrder::IngressFirst,
        });
        id
    }

    /// Withdraws a transaction-local mapping before active publication.
    ///
    /// Interface IDs remain monotonic and are not reused. Runtime detach is not
    /// part of R0; the kernel attach authority only uses this for rollback when
    /// worker/wake/time preparation fails.
    pub fn remove_interface(&mut self, id: InterfaceId) -> Result<(), PumpError> {
        let Some(index) = self.interfaces.iter().position(|entry| entry.id == id) else {
            return Err(PumpError::UnknownInterface(id));
        };
        let mut entry = self.interfaces.remove(index);
        self.udp.remove_interface(id, &mut entry.sockets);
        Ok(())
    }

    #[cfg(any(test, feature = "host-test"))]
    pub(crate) fn interface_mut(
        &mut self,
        id: InterfaceId,
    ) -> Result<&mut InterfaceEntry, PumpError> {
        self.interfaces
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or(PumpError::UnknownInterface(id))
    }
}
