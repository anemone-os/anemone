//! Initial-domain protocol-Stack mapping and pump capabilities.

use anemone_net_api::{
    EthernetAddress, FrameProvider, Instant as NetworkInstant, InterfaceId, Ipv4Address, Ipv4Cidr,
    PumpOutcome, icmp_raw::IcmpRawNamespacePolicy, udp::UdpNamespacePolicy,
};
use anemone_smoltcp_stack::{
    Ipv4ConfigError, PumpBudget, PumpError, Stack, StackPolicy, TcpPolicy,
};

use crate::prelude::*;

mod icmp_raw;
mod tcp;
mod udp;

use icmp_raw::IcmpRawEndpointEventRoutes;
use tcp::TcpEndpointEventRoutes;
use udp::UdpEndpointEventRoutes;

struct RecheckRoute<Id, Observer: ?Sized> {
    endpoint: Id,
    observer: Weak<Observer>,
}

struct RecheckRoutes<Id, Observer: ?Sized> {
    routes: Vec<RecheckRoute<Id, Observer>>,
}

impl<Id: Copy + Eq, Observer: ?Sized> RecheckRoutes<Id, Observer> {
    const fn new() -> Self {
        Self { routes: Vec::new() }
    }

    fn register(
        &mut self,
        endpoint: Id,
        observer: &Arc<Observer>,
    ) -> Result<(), crate::net::EventRegistrationError> {
        assert!(
            self.routes.iter().all(|route| route.endpoint != endpoint),
            "one Endpoint cannot publish two reverse event routes"
        );
        self.routes
            .try_reserve(1)
            .map_err(|_| crate::net::EventRegistrationError::OutOfMemory)?;
        self.routes.push(RecheckRoute {
            endpoint,
            observer: Arc::downgrade(observer),
        });
        Ok(())
    }

    fn unregister(&mut self, endpoint: Id) {
        let index = self
            .routes
            .iter()
            .position(|route| route.endpoint == endpoint)
            .expect("published Endpoint event route disappeared before unregister");
        self.routes.remove(index);
    }

    fn observer(&mut self, endpoint: Id) -> Option<Weak<Observer>> {
        // Pruning is resource hygiene only. Correctness comes from explicit
        // source unregister plus a fresh facts snapshot after every hint.
        self.routes
            .retain(|route| route.observer.strong_count() != 0);
        self.routes
            .iter()
            .find(|route| route.endpoint == endpoint)
            .map(|route| route.observer.clone())
    }
}

pub(in crate::net) struct DomainStack {
    stack: SpinLock<Stack>,
    icmp_raw_event_routes: SpinLock<IcmpRawEndpointEventRoutes>,
    tcp_event_routes: SpinLock<TcpEndpointEventRoutes>,
    udp_event_routes: SpinLock<UdpEndpointEventRoutes>,
}

impl DomainStack {
    pub(super) fn new(
        udp_policy: UdpNamespacePolicy,
        icmp_raw_policy: IcmpRawNamespacePolicy,
        tcp_policy: TcpPolicy,
    ) -> Self {
        Self {
            stack: SpinLock::new(Stack::with_policy(StackPolicy::new(
                udp_policy,
                icmp_raw_policy,
                tcp_policy,
            ))),
            icmp_raw_event_routes: SpinLock::new(IcmpRawEndpointEventRoutes::new()),
            tcp_event_routes: SpinLock::new(TcpEndpointEventRoutes::new()),
            udp_event_routes: SpinLock::new(UdpEndpointEventRoutes::new()),
        }
    }

    fn protocol_transition<T>(&self, operation: impl FnOnce(&mut Stack) -> T) -> T {
        let (result, udp_invalidations, icmp_raw_invalidations, tcp_invalidations) = {
            let mut stack = self.stack.lock();
            let result = operation(&mut stack);
            let (udp_invalidations, icmp_raw_invalidations, tcp_invalidations) =
                stack.take_invalidations().into_parts();
            (
                result,
                udp_invalidations,
                icmp_raw_invalidations,
                tcp_invalidations,
            )
        };
        // Endpoint owners commit facts under the Stack guard. Recheck-only
        // hints cross into kernel observers only after that guard is gone.
        self.route_udp_invalidations(udp_invalidations);
        self.route_icmp_raw_invalidations(icmp_raw_invalidations);
        self.route_tcp_invalidations(tcp_invalidations);
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

    pub(super) fn pump_local(
        &self,
        interface: InterfaceId,
        now: NetworkInstant,
        budget: PumpBudget,
    ) -> Result<PumpOutcome, PumpError> {
        self.protocol_transition(|stack| stack.pump_local(interface, now, budget))
    }

    pub(super) fn pump_external<P: FrameProvider>(
        &self,
        interface: InterfaceId,
        provider: &mut P,
        now: NetworkInstant,
        budget: PumpBudget,
    ) -> Result<PumpOutcome, PumpError> {
        self.protocol_transition(|stack| stack.pump(interface, provider, now, budget))
    }

    pub(super) fn rollback_external_mapping(
        &self,
        interface: InterfaceId,
    ) -> Result<(), PumpError> {
        self.protocol_transition(|stack| stack.remove_interface(interface))
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

#[cfg(feature = "kunit")]
mod kunits {
    use anemone_net_api::{icmp_raw::IcmpRawEndpointId, tcp::TcpEndpointId, udp::UdpEndpointId};

    use super::*;
    use crate::net::{
        icmp_raw::IcmpRawEndpointInvalidationObserver, tcp::TcpEndpointInvalidationObserver,
        udp::UdpEndpointInvalidationObserver,
    };

    struct Observer;

    impl UdpEndpointInvalidationObserver for Observer {
        fn invalidate(&self) {}
    }

    impl IcmpRawEndpointInvalidationObserver for Observer {
        fn invalidate(&self) {}
    }

    impl TcpEndpointInvalidationObserver for Observer {
        fn invalidate(&self) {}
    }

    #[kunit]
    fn typed_recheck_routes_share_storage_rules_without_sharing_endpoint_truth() {
        let udp_id = UdpEndpointId::from_owner_raw(11);
        let udp_observer: Arc<dyn UdpEndpointInvalidationObserver> = Arc::new(Observer);
        let mut udp_routes =
            RecheckRoutes::<UdpEndpointId, dyn UdpEndpointInvalidationObserver>::new();
        udp_routes.register(udp_id, &udp_observer).unwrap();
        assert!(udp_routes.observer(udp_id).unwrap().upgrade().is_some());
        udp_routes.unregister(udp_id);
        assert!(udp_routes.observer(udp_id).is_none());

        let icmp_id = IcmpRawEndpointId::from_owner_raw(12);
        let icmp_observer: Arc<dyn IcmpRawEndpointInvalidationObserver> = Arc::new(Observer);
        let mut icmp_routes =
            RecheckRoutes::<IcmpRawEndpointId, dyn IcmpRawEndpointInvalidationObserver>::new();
        icmp_routes.register(icmp_id, &icmp_observer).unwrap();
        drop(icmp_observer);
        assert!(icmp_routes.observer(icmp_id).is_none());

        let tcp_id = TcpEndpointId::from_owner_raw(13);
        let tcp_observer: Arc<dyn TcpEndpointInvalidationObserver> = Arc::new(Observer);
        let mut tcp_routes =
            RecheckRoutes::<TcpEndpointId, dyn TcpEndpointInvalidationObserver>::new();
        tcp_routes.register(tcp_id, &tcp_observer).unwrap();
        assert!(tcp_routes.observer(tcp_id).unwrap().upgrade().is_some());
        tcp_routes.unregister(tcp_id);
        assert!(tcp_routes.observer(tcp_id).is_none());
    }
}
