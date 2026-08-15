//! Bounded progression for the production IP-medium local port.

use anemone_net_api::{Instant, InterfaceId, PumpOutcome};
use smoltcp::iface::{PollIngressSingleResult, PollResult};

use crate::{
    adapter::{from_smoltcp_instant, to_smoltcp_instant},
    local_link::{LocalDevice, packet_owner},
    stack::{Protocols, PumpError, PumpOrder, Stack},
};

use super::common::{PumpBudget, RoundContinuation, pump_outcome};

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
        let active = self
            .protocols
            .prepare_egress(id, &mut local.protocols, &mut local.sockets);
        local.link.set_tx_owner(packet_owner(active));

        let mut device = LocalDevice::new(&mut local.link);
        let (ingress_may_remain, egress_may_remain) = match local.next_pump_order {
            PumpOrder::IngressFirst => (
                poll_local_ingress(
                    id,
                    &mut local.interface,
                    &mut local.sockets,
                    &mut self.protocols,
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
                    &mut self.protocols,
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
        let protocol_egress_may_remain =
            self.protocols
                .complete_egress(active, id, &local.protocols, &local.sockets);
        let tcp_progression = self.protocols.reclaim_tcp(id, &mut local.sockets);
        // Match external pump semantics: timers can mutate endpoint facts even
        // when the device cannot emit the packet that would report a change.
        self.protocols.invalidate_tcp_interface(id);
        local.next_pump_order = local.next_pump_order.next();

        let next_deadline = local
            .interface
            .poll_at(smoltcp_now, &local.sockets)
            .map(from_smoltcp_instant);
        // Local capacity never waits on another owner: the next pump can
        // consume normal ingress and free the same bounded link. A transmit
        // rejected earlier in this round therefore remains runnable even if
        // another progress signal was not observed after ingress.
        let continuation = if device_blocked
            || ingress_may_remain
            || egress_may_remain
            || protocol_egress_may_remain
            || tcp_progression
            || transferred != 0
        {
            RoundContinuation::Runnable
        } else {
            RoundContinuation::Quiescent
        };
        Ok(pump_outcome(continuation, now, next_deadline))
    }
}

fn poll_local_ingress(
    id: InterfaceId,
    interface: &mut smoltcp::iface::Interface,
    sockets: &mut smoltcp::iface::SocketSet<'static>,
    protocols: &mut Protocols,
    device: &mut LocalDevice<'_>,
    now: smoltcp::time::Instant,
    budget: usize,
) -> bool {
    protocols.drain_engine_ingress(id, sockets);
    let mut processed = 0;
    while processed < budget {
        let result =
            interface.poll_ingress_single_with_ipv4_observer(now, device, sockets, &mut |packet| {
                protocols.observe_admitted_ipv4(packet)
            });
        match result {
            PollIngressSingleResult::None => break,
            PollIngressSingleResult::PacketProcessed
            | PollIngressSingleResult::SocketStateChanged => processed += 1,
        }
        protocols.drain_engine_ingress(id, sockets);
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
