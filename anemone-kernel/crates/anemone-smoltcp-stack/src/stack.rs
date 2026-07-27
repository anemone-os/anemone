use alloc::vec::Vec;

use anemone_net_api::{EthernetAddress, FrameProvider, Instant, InterfaceId};
use smoltcp::{
    iface::{Config, Interface, SocketSet},
    wire::{EthernetAddress as SmoltcpEthernetAddress, HardwareAddress},
};

use crate::adapter::FrameDevice;
#[cfg(feature = "icmp-validation-probe")]
use crate::validation::IcmpEchoProbe;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PumpError {
    UnknownInterface(InterfaceId),
}

pub(crate) struct InterfaceEntry {
    pub(crate) id: InterfaceId,
    // Stable attach-time snapshot required by smoltcp. The provider remains
    // the truth source; every pump asserts that this snapshot is not stale.
    pub(crate) frame_capacity: usize,
    pub(crate) interface: Interface,
    pub(crate) sockets: SocketSet<'static>,
    #[cfg(feature = "icmp-validation-probe")]
    pub(crate) validation_probe: Option<IcmpEchoProbe>,
}

/// Owns the private smoltcp interface resources and their opaque ID mapping.
///
/// `&mut Stack` is the unique pump capability. The kernel wiring owner may put
/// the stack behind its chosen synchronization primitive, but admission,
/// contention, and requeue policy stay outside this protocol-state owner.
#[derive(Default)]
pub struct Stack {
    interfaces: Vec<InterfaceEntry>,
    next_interface_id: u32,
}

impl Stack {
    pub const fn new() -> Self {
        Self {
            interfaces: Vec::new(),
            next_interface_id: 0,
        }
    }

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

        self.interfaces.push(InterfaceEntry {
            id,
            frame_capacity,
            interface,
            sockets: SocketSet::new(Vec::new()),
            #[cfg(feature = "icmp-validation-probe")]
            validation_probe: None,
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
        self.interfaces.remove(index);
        Ok(())
    }

    pub(crate) fn interface_mut(
        &mut self,
        id: InterfaceId,
    ) -> Result<&mut InterfaceEntry, PumpError> {
        self.interfaces
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or(PumpError::UnknownInterface(id))
    }

    /// Installs an IPv4 address solely for the deterministic host fixture.
    ///
    /// `host-test` is absent from the kernel dependency, so this control cannot
    /// become a production address API. Remove it when the fixture can stay
    /// crate-private or an accepted control-plane owner replaces it.
    #[cfg(feature = "host-test")]
    pub fn configure_ipv4_for_host_validation(
        &mut self,
        id: InterfaceId,
        address: [u8; 4],
        prefix_len: u8,
    ) -> Result<(), PumpError> {
        use smoltcp::wire::{IpAddress, IpCidr, Ipv4Address};

        let entry = self.interface_mut(id)?;
        let cidr = IpCidr::new(
            IpAddress::Ipv4(Ipv4Address::from_octets(address)),
            prefix_len,
        );
        entry.interface.update_ip_addrs(|addresses| {
            addresses.clear();
            assert!(addresses.push(cidr).is_ok());
        });
        Ok(())
    }

    /// Queues complete IPv4 packets solely for the deterministic host fixture.
    ///
    /// The socket and its handle stay private to this stack owner. `host-test`
    /// is absent from the kernel dependency, so this cannot become a
    /// production packet-injection or control-plane API.
    #[cfg(feature = "host-test")]
    pub fn queue_ipv4_for_host_validation(
        &mut self,
        id: InterfaceId,
        packets: &[&[u8]],
    ) -> Result<(), PumpError> {
        use alloc::vec;
        use smoltcp::{socket::raw, wire::IpVersion};

        let entry = self.interface_mut(id)?;
        let payload_capacity = packets
            .iter()
            .try_fold(0usize, |total, packet| total.checked_add(packet.len()))
            .expect("host validation packet storage capacity overflow");
        let mut socket = raw::Socket::new(
            Some(IpVersion::Ipv4),
            None,
            raw::PacketBuffer::new(vec![raw::PacketMetadata::EMPTY], vec![0; 1]),
            raw::PacketBuffer::new(
                vec![raw::PacketMetadata::EMPTY; packets.len()],
                vec![0; payload_capacity],
            ),
        );
        for packet in packets {
            socket
                .send_slice(packet)
                .expect("sized host validation socket must accept every packet");
        }
        entry.sockets.add(socket);
        Ok(())
    }
}
