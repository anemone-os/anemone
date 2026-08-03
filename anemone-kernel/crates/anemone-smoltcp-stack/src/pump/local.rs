//! Bounded progression for the production IP-medium local port.

use anemone_net_api::{Instant, InterfaceId, PumpOutcome};
use smoltcp::iface::{PollIngressSingleResult, PollResult};

use crate::{
    adapter::{from_smoltcp_instant, to_smoltcp_instant},
    icmp_raw::IcmpRawEndpoints,
    local_link::{LocalDevice, PacketOwner},
    stack::{PumpError, PumpOrder, Stack},
    udp::UdpEndpoints,
};

use super::common::{PumpBudget, prepare_protocol_egress, pump_outcome};

impl Stack {
    /// Advances the production IP-medium local port with the same exclusive
    /// `&mut Stack` capability as external interfaces. Egress is transferred
    /// into bounded link storage only after the protocol round, so normal
    /// ingress cannot consume it until a later bounded call.
    pub fn pump_local(
        &mut self,
        id: InterfaceId,
        now: Instant,
        budget: PumpBudget,
    ) -> Result<PumpOutcome, PumpError> {
        let local = self
            .local
            .as_mut()
            .filter(|local| local.id == id)
            .ok_or(PumpError::UnknownInterface(id))?;
        let smoltcp_now = to_smoltcp_instant(now);
        local.interface.poll_maintenance(smoltcp_now);
        let active = prepare_protocol_egress(
            id,
            &mut local.sockets,
            local.icmp_raw_engine,
            &mut local.next_egress_protocol,
            &mut self.icmp_raw,
            &mut self.udp,
        );
        local.link.set_tx_owner(
            active
                .icmp_raw
                .map(PacketOwner::IcmpRaw)
                .or_else(|| active.udp.map(PacketOwner::Udp)),
        );

        let mut device = LocalDevice::new(&mut local.link);
        let (ingress_may_remain, egress_may_remain) = match local.next_pump_order {
            PumpOrder::IngressFirst => (
                poll_local_ingress(
                    id,
                    &mut local.interface,
                    &mut local.sockets,
                    local.icmp_raw_engine,
                    &mut self.icmp_raw,
                    &mut self.udp,
                    &mut device,
                    smoltcp_now,
                    budget.ingress_frames(),
                ),
                poll_local_egress(
                    &mut local.interface,
                    &mut local.sockets,
                    &mut device,
                    smoltcp_now,
                    budget.egress_steps(),
                ),
            ),
            PumpOrder::EgressFirst => {
                let egress_may_remain = poll_local_egress(
                    &mut local.interface,
                    &mut local.sockets,
                    &mut device,
                    smoltcp_now,
                    budget.egress_steps(),
                );
                let ingress_may_remain = poll_local_ingress(
                    id,
                    &mut local.interface,
                    &mut local.sockets,
                    local.icmp_raw_engine,
                    &mut self.icmp_raw,
                    &mut self.udp,
                    &mut device,
                    smoltcp_now,
                    budget.ingress_frames(),
                );
                (ingress_may_remain, egress_may_remain)
            },
        };
        let device_blocked = device.blocked_work();
        drop(device);
        local.link.set_tx_owner(None);
        // Transfer publishes new normal-ingress work only after this protocol
        // round has finished. Even when the egress loop stopped below its
        // budget, a non-empty transfer therefore requires one later bounded
        // round; otherwise a sleeping production worker can strand the packet
        // without another owner capable of issuing a wake.
        let transferred = local.link.transfer(budget.ingress_frames());
        let owner_blocked = device_blocked && local.link.occupied() >= local.local_link_capacity();
        let udp_egress_may_remain = self.udp.complete_egress(active.udp, id, &local.sockets);
        let icmp_raw_egress_may_remain =
            self.icmp_raw
                .complete_egress(id, local.icmp_raw_engine, &local.sockets);
        local.next_pump_order = local.next_pump_order.next();

        let next_deadline = local
            .interface
            .poll_at(smoltcp_now, &local.sockets)
            .map(from_smoltcp_instant);
        Ok(pump_outcome(
            owner_blocked,
            ingress_may_remain,
            egress_may_remain
                || udp_egress_may_remain
                || icmp_raw_egress_may_remain
                || transferred != 0,
            now,
            next_deadline,
        ))
    }
}

fn poll_local_ingress(
    id: InterfaceId,
    interface: &mut smoltcp::iface::Interface,
    sockets: &mut smoltcp::iface::SocketSet<'static>,
    icmp_raw_engine: crate::icmp_raw::namespace::EngineResource,
    icmp_raw: &mut IcmpRawEndpoints,
    udp: &mut UdpEndpoints,
    device: &mut LocalDevice<'_>,
    now: smoltcp::time::Instant,
    budget: usize,
) -> bool {
    icmp_raw.drain_ingress(icmp_raw_engine, sockets);
    udp.drain_ingress(id, sockets);
    let mut processed = 0;
    while processed < budget {
        match interface.poll_ingress_single(now, device, sockets) {
            PollIngressSingleResult::None => break,
            PollIngressSingleResult::PacketProcessed
            | PollIngressSingleResult::SocketStateChanged => processed += 1,
        }
        icmp_raw.drain_ingress(icmp_raw_engine, sockets);
        udp.drain_ingress(id, sockets);
    }
    processed == budget
}

fn poll_local_egress(
    interface: &mut smoltcp::iface::Interface,
    sockets: &mut smoltcp::iface::SocketSet<'static>,
    device: &mut LocalDevice<'_>,
    now: smoltcp::time::Instant,
    budget: usize,
) -> bool {
    let mut processed = 0;
    while processed < budget {
        match interface.poll_egress(now, device, sockets) {
            PollResult::None => break,
            PollResult::SocketStateChanged => processed += 1,
        }
    }
    processed == budget
}
