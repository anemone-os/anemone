//! Initial-domain protocol-Stack mapping and pump capabilities.

use anemone_net_api::{
    EthernetAddress, FrameProvider, Instant as NetworkInstant, InterfaceId, Ipv4Address, Ipv4Cidr,
    PumpOutcome, icmp_raw::IcmpRawNamespacePolicy, udp::UdpNamespacePolicy,
};
use anemone_smoltcp_stack::{Ipv4ConfigError, PumpBudget, PumpError, Stack};

use crate::prelude::*;

mod icmp_raw;
mod udp;

use icmp_raw::IcmpRawEndpointEventRoutes;
use udp::UdpEndpointEventRoutes;

pub(in crate::net) struct DomainStack {
    stack: SpinLock<Stack>,
    icmp_raw_event_routes: SpinLock<IcmpRawEndpointEventRoutes>,
    event_routes: SpinLock<UdpEndpointEventRoutes>,
}

impl DomainStack {
    pub(super) fn new(
        udp_policy: UdpNamespacePolicy,
        icmp_raw_policy: IcmpRawNamespacePolicy,
    ) -> Self {
        Self {
            stack: SpinLock::new(Stack::new_with_namespace_policies(
                udp_policy,
                icmp_raw_policy,
            )),
            icmp_raw_event_routes: SpinLock::new(IcmpRawEndpointEventRoutes::new()),
            event_routes: SpinLock::new(UdpEndpointEventRoutes::new()),
        }
    }

    fn protocol_transition<T>(&self, operation: impl FnOnce(&mut Stack) -> T) -> T {
        let (result, udp_invalidations, icmp_raw_invalidations) = {
            let mut stack = self.stack.lock();
            let result = operation(&mut stack);
            let udp_invalidations = stack.take_udp_endpoint_invalidations();
            let icmp_raw_invalidations = stack.take_icmp_raw_endpoint_invalidations();
            (result, udp_invalidations, icmp_raw_invalidations)
        };
        // Endpoint owners commit facts under the Stack guard. Recheck-only
        // hints cross into kernel observers only after that guard is gone.
        self.route_udp_invalidations(udp_invalidations);
        self.route_icmp_raw_invalidations(icmp_raw_invalidations);
        result
    }

    pub(super) fn attach_local(
        self: &Arc<Self>,
        now: NetworkInstant,
    ) -> Result<LocalPumpPort, Ipv4ConfigError> {
        let loopback = Ipv4Cidr::new(Ipv4Address::LOOPBACK, 8).expect("/8 is valid");
        let interface = self.stack.lock().add_local_ipv4(
            loopback,
            NET_LOCAL_LINK_PACKET_CAPACITY,
            NET_LOCAL_LINK_MTU_BYTES,
            now,
        )?;
        Ok(LocalPumpPort {
            stack: self.clone(),
            interface,
        })
    }

    pub(super) fn install_ipv4_projection(
        &self,
        external: Option<(InterfaceId, Ipv4Cidr, Option<Ipv4Address>)>,
    ) -> Result<(), Ipv4ConfigError> {
        let mut stack = self.stack.lock();
        if let Some((interface, cidr, gateway)) = external {
            stack.configure_external_ipv4(interface, cidr, gateway)?;
            stack.add_local_delivery_ipv4(cidr.address())?;
        }
        Ok(())
    }

    pub(in crate::net) fn attach_external<P: FrameProvider>(
        self: &Arc<Self>,
        provider: &mut P,
        ethernet_address: EthernetAddress,
        now: NetworkInstant,
    ) -> ExternalMapping {
        let interface = self
            .stack
            .lock()
            .add_interface(provider, ethernet_address, now);
        ExternalMapping {
            stack: self.clone(),
            interface,
            finished: false,
        }
    }
}

/// Transaction-local mapping owner used only before active publication.
pub(in crate::net) struct ExternalMapping {
    stack: Arc<DomainStack>,
    interface: InterfaceId,
    finished: bool,
}

impl ExternalMapping {
    pub(in crate::net) fn pump_port(&self) -> ExternalPumpPort {
        ExternalPumpPort {
            stack: self.stack.clone(),
            interface: self.interface,
        }
    }

    pub(in crate::net) const fn interface(&self) -> InterfaceId {
        self.interface
    }

    pub(in crate::net) fn commit(mut self) {
        self.finished = true;
    }

    pub(in crate::net) fn rollback(mut self) {
        let removed = self.stack.rollback_external_mapping(self.interface);
        self.finished = true;
        removed.expect("failed attach lost its global-Stack mapping");
    }
}

impl Drop for ExternalMapping {
    fn drop(&mut self) {
        if self.finished {
            return;
        }

        // Fail closed before reporting the protocol bug: this mapping has not
        // been published active, so leaving it behind would let a panic turn
        // an attach mistake into stale global-Stack state.
        let removed = self.stack.rollback_external_mapping(self.interface);
        self.finished = true;
        removed.expect("unfinished external mapping was already absent");
        panic!("external Stack mapping dropped without commit or rollback");
    }
}

/// Worker-local capability for one interface on the domain Stack.
pub(in crate::net) struct ExternalPumpPort {
    stack: Arc<DomainStack>,
    interface: InterfaceId,
}

/// Worker-local capability for the one initial-domain software interface.
pub(in crate::net) struct LocalPumpPort {
    stack: Arc<DomainStack>,
    interface: InterfaceId,
}

impl LocalPumpPort {
    pub(in crate::net) const fn interface(&self) -> InterfaceId {
        self.interface
    }

    pub(in crate::net) fn pump(
        &self,
        now: NetworkInstant,
        budget: PumpBudget,
    ) -> Result<PumpOutcome, PumpError> {
        self.stack.pump_local(self.interface, now, budget)
    }
}

impl ExternalPumpPort {
    pub(in crate::net) fn pump<P: FrameProvider>(
        &self,
        provider: &mut P,
        now: NetworkInstant,
        budget: PumpBudget,
    ) -> Result<PumpOutcome, PumpError> {
        // A provider callback runs only inside this finite pump window. It must
        // not sleep or re-enter the domain/attach owners.
        self.stack
            .pump_external(self.interface, provider, now, budget)
    }
}
