use alloc::{collections::VecDeque, vec::Vec};

use anemone_net_api::icmp_raw::{
    IcmpRawAssociation, IcmpRawDropDiagnostics, IcmpRawEndpointConfig, IcmpRawEndpointFacts,
    IcmpRawEndpointId, IcmpRawEndpointLimits, IcmpRawTypeFilter,
};

use super::packet::PendingPacket;

pub(super) struct Endpoint {
    pub(super) id: IcmpRawEndpointId,
    local: LocalAssociation,
    peer: Option<anemone_net_api::Ipv4Address>,
    pub(super) filter: IcmpRawTypeFilter,
    pub(super) limits: IcmpRawEndpointLimits,
    pub(super) received: VecDeque<Vec<u8>>,
    // O(1) hot-path cache of the total bytes in `received`. The queue is the
    // truth source; this value is updated in the same owner transition and is
    // never allowed to be stale.
    pub(super) rx_bytes: usize,
    pub(super) pending_tx: VecDeque<PendingPacket>,
    // O(1) hot-path cache of bytes in `pending_tx` plus owner-active egress
    // records. Those packet records are the truth source; every handoff and
    // completion updates this value in the same owner transition.
    pub(super) tx_bytes: usize,
    // Diagnostic-only counters. Admission decisions use the queues and limits
    // above; stale or saturated counters never feed behavior back into them.
    pub(super) dropped_rx_packets: u64,
    pub(super) dropped_rx_bytes: u64,
}

/// Only connect-selected sources are cleared by disconnect. The Stack must
/// retain this origin because the public address snapshot alone cannot
/// distinguish it from an explicit bind to the same address.
#[derive(Clone, Copy)]
enum LocalAssociation {
    Unbound,
    ConnectSelected(anemone_net_api::Ipv4Address),
    Explicit(anemone_net_api::Ipv4Address),
}

impl LocalAssociation {
    const fn address(self) -> Option<anemone_net_api::Ipv4Address> {
        match self {
            Self::Unbound => None,
            Self::ConnectSelected(address) | Self::Explicit(address) => Some(address),
        }
    }
}

impl Endpoint {
    pub(super) fn new(id: IcmpRawEndpointId, limits: IcmpRawEndpointLimits) -> Self {
        assert!(limits.tx_packet_capacity() > 0);
        assert!(limits.tx_byte_capacity() >= super::packet::IPV4_HEADER_LEN);
        assert!(limits.rx_packet_capacity() > 0);
        assert!(limits.rx_byte_capacity() >= super::packet::IPV4_HEADER_LEN);
        Self {
            id,
            local: LocalAssociation::Unbound,
            peer: None,
            filter: IcmpRawTypeFilter::default(),
            limits,
            received: VecDeque::with_capacity(limits.rx_packet_capacity()),
            rx_bytes: 0,
            pending_tx: VecDeque::with_capacity(limits.tx_packet_capacity()),
            tx_bytes: 0,
            dropped_rx_packets: 0,
            dropped_rx_bytes: 0,
        }
    }

    pub(super) fn facts(&self, active_packets: usize) -> IcmpRawEndpointFacts {
        let tx_packets = self.pending_tx.len() + active_packets;
        let writable = tx_packets < self.limits.tx_packet_capacity()
            && self
                .tx_bytes
                .checked_add(super::packet::IPV4_HEADER_LEN)
                .is_some_and(|bytes| bytes <= self.limits.tx_byte_capacity());
        IcmpRawEndpointFacts::from_owner_snapshot(self.received.front().map(Vec::len), writable)
    }

    pub(super) fn config(&self) -> IcmpRawEndpointConfig {
        IcmpRawEndpointConfig::from_owner_snapshot(
            IcmpRawAssociation::new(self.local.address(), self.peer),
            self.filter,
        )
    }

    pub(super) fn bind(&mut self, local: Option<anemone_net_api::Ipv4Address>) -> Result<(), ()> {
        if self.peer.is_some() {
            return Err(());
        }
        self.local = local.map_or(LocalAssociation::Unbound, LocalAssociation::Explicit);
        Ok(())
    }

    pub(super) fn connect(
        &mut self,
        selected_source: anemone_net_api::Ipv4Address,
        peer: anemone_net_api::Ipv4Address,
    ) {
        if !matches!(self.local, LocalAssociation::Explicit(_)) {
            self.local = LocalAssociation::ConnectSelected(selected_source);
        }
        self.peer = Some(peer);
    }

    pub(super) fn disconnect(&mut self) {
        if matches!(self.local, LocalAssociation::ConnectSelected(_)) {
            self.local = LocalAssociation::Unbound;
        }
        self.peer = None;
    }

    pub(super) fn diagnostics(&self) -> IcmpRawDropDiagnostics {
        IcmpRawDropDiagnostics::from_owner_snapshot(self.dropped_rx_packets, self.dropped_rx_bytes)
    }

    pub(super) fn matches(
        &self,
        source: anemone_net_api::Ipv4Address,
        destination: anemone_net_api::Ipv4Address,
        icmp_type: Option<u8>,
    ) -> bool {
        self.local
            .address()
            .is_none_or(|local| local == destination)
            && self.peer.is_none_or(|peer| peer == source)
            && icmp_type.is_none_or(|kind| self.filter.allows(kind))
    }

    pub(super) fn admit_rx(&mut self, packet: &[u8]) -> bool {
        let was_empty = self.received.is_empty();
        let byte_capacity = self
            .rx_bytes
            .checked_add(packet.len())
            .is_some_and(|bytes| bytes <= self.limits.rx_byte_capacity());
        if self.received.len() >= self.limits.rx_packet_capacity() || !byte_capacity {
            self.dropped_rx_packets = self.dropped_rx_packets.saturating_add(1);
            self.dropped_rx_bytes = self.dropped_rx_bytes.saturating_add(packet.len() as u64);
            return false;
        }
        self.received.push_back(packet.to_vec());
        self.rx_bytes += packet.len();
        was_empty
    }
}
