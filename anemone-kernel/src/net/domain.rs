//! Initial network-domain identity and protocol-Stack owners.

use anemone_net_api::{
    EthernetAddress, FrameProvider, Instant as NetworkInstant, InterfaceId, PumpOutcome,
};
use anemone_smoltcp_stack::{PumpBudget, PumpError, Stack};

use crate::{device::net::NetdevId, prelude::*, utils::identity::GeneralIdentity};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub(super) struct LogicalInterfaceId(u32);

impl LogicalInterfaceId {
    pub(super) const fn index(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LogicalInterfaceKind {
    Loopback,
    External,
}

/// Immutable domain-membership fact.
///
/// `netdev` is an opaque association for external-interface diagnosis; it is
/// never used to derive the logical or protocol identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LogicalInterfaceSnapshot {
    id: LogicalInterfaceId,
    ifindex: u32,
    name: GeneralIdentity,
    kind: LogicalInterfaceKind,
    netdev: Option<NetdevId>,
}

impl LogicalInterfaceSnapshot {
    pub(super) const fn id(&self) -> LogicalInterfaceId {
        self.id
    }

    pub(super) const fn ifindex(&self) -> u32 {
        self.ifindex
    }

    pub(super) fn name(&self) -> &str {
        self.name.as_str()
    }

    pub(super) const fn kind(&self) -> LogicalInterfaceKind {
        self.kind
    }

    pub(super) const fn netdev(&self) -> Option<NetdevId> {
        self.netdev
    }
}

pub(super) struct LogicalInterfaceReservation {
    snapshot: LogicalInterfaceSnapshot,
    finished: bool,
}

impl LogicalInterfaceReservation {
    pub(super) fn snapshot(&self) -> &LogicalInterfaceSnapshot {
        &self.snapshot
    }
}

impl Drop for LogicalInterfaceReservation {
    fn drop(&mut self) {
        // Reservation consumes its identities immediately so an aborted or
        // buggy attach can never reuse them. There is no published membership
        // to withdraw here; commit/abort only closes the transaction token.
        assert!(
            self.finished,
            "logical-interface reservation dropped without commit or abort"
        );
    }
}

/// Sole owner of initial-domain membership, identity, ifindex, name, and kind.
pub(super) struct LogicalInterfaces {
    members: Vec<LogicalInterfaceSnapshot>,
    next_id: u32,
    next_ifindex: u32,
    next_external_ordinal: u32,
}

impl LogicalInterfaces {
    fn new() -> Self {
        let loopback = LogicalInterfaceSnapshot {
            id: LogicalInterfaceId(0),
            ifindex: 1,
            name: GeneralIdentity::try_from("lo").expect("loopback name must fit"),
            kind: LogicalInterfaceKind::Loopback,
            netdev: None,
        };
        Self {
            members: vec![loopback],
            next_id: 1,
            next_ifindex: 2,
            next_external_ordinal: 0,
        }
    }

    pub(super) fn reserve_external(&mut self, netdev: NetdevId) -> LogicalInterfaceReservation {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .expect("logical-interface identity space exhausted");
        let ifindex = self.next_ifindex;
        self.next_ifindex = ifindex
            .checked_add(1)
            .expect("logical-interface ifindex space exhausted");
        let ordinal = self.next_external_ordinal;
        self.next_external_ordinal = ordinal
            .checked_add(1)
            .expect("external-interface ordinal space exhausted");
        let name = GeneralIdentity::try_from_fmt(format_args!("eth{ordinal}"))
            .expect("external-interface name must fit");

        LogicalInterfaceReservation {
            snapshot: LogicalInterfaceSnapshot {
                id: LogicalInterfaceId(id),
                ifindex,
                name,
                kind: LogicalInterfaceKind::External,
                netdev: Some(netdev),
            },
            finished: false,
        }
    }

    pub(super) fn commit(
        &mut self,
        mut reservation: LogicalInterfaceReservation,
    ) -> LogicalInterfaceSnapshot {
        let snapshot = reservation.snapshot.clone();
        assert!(
            self.members.iter().all(|member| member.id != snapshot.id),
            "logical-interface reservation committed twice"
        );
        reservation.finished = true;
        self.members.push(snapshot.clone());
        snapshot
    }

    pub(super) fn abort(&mut self, mut reservation: LogicalInterfaceReservation) {
        assert!(
            self.members
                .iter()
                .all(|member| member.id != reservation.snapshot.id),
            "published logical interface cannot be aborted"
        );
        reservation.finished = true;
    }

    fn loopback(&self) -> &LogicalInterfaceSnapshot {
        let loopback = &self.members[0];
        assert_eq!(loopback.kind, LogicalInterfaceKind::Loopback);
        loopback
    }
}

pub(super) struct DomainStack {
    stack: SpinLock<Stack>,
}

impl DomainStack {
    fn new() -> Self {
        Self {
            stack: SpinLock::new(Stack::new()),
        }
    }

    pub(super) fn attach_external<P: FrameProvider>(
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
pub(super) struct ExternalMapping {
    stack: Arc<DomainStack>,
    interface: InterfaceId,
    finished: bool,
}

impl ExternalMapping {
    pub(super) fn pump_port(&self) -> ExternalPumpPort {
        ExternalPumpPort {
            stack: self.stack.clone(),
            interface: self.interface,
        }
    }

    pub(super) const fn interface(&self) -> InterfaceId {
        self.interface
    }

    pub(super) fn commit(mut self) {
        self.finished = true;
    }

    pub(super) fn rollback(mut self) {
        let removed = self.stack.stack.lock().remove_interface(self.interface);
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
        let removed = self.stack.stack.lock().remove_interface(self.interface);
        self.finished = true;
        removed.expect("unfinished external mapping was already absent");
        panic!("external Stack mapping dropped without commit or rollback");
    }
}

/// Worker-local capability for one interface on the domain Stack.
pub(super) struct ExternalPumpPort {
    stack: Arc<DomainStack>,
    interface: InterfaceId,
}

impl ExternalPumpPort {
    pub(super) fn pump<P: FrameProvider>(
        &self,
        provider: &mut P,
        now: NetworkInstant,
        budget: PumpBudget,
    ) -> Result<PumpOutcome, PumpError> {
        // A provider callback runs only inside this finite pump window. It must
        // not sleep or re-enter the domain/attach owners.
        self.stack
            .stack
            .lock()
            .pump(self.interface, provider, now, budget)
    }
}

/// Boot-persistent composition of the two distinct initial-domain owners.
pub(super) struct InitialDomain {
    logical: LogicalInterfaces,
    stack: Arc<DomainStack>,
}

impl InitialDomain {
    pub(super) fn new() -> Self {
        let domain = Self {
            logical: LogicalInterfaces::new(),
            stack: Arc::new(DomainStack::new()),
        };
        let loopback = domain.logical.loopback();
        kinfoln!(
            "initial network domain: {} (ifindex {}, {:?}) committed; global protocol Stack initialized",
            loopback.name(),
            loopback.ifindex(),
            loopback.kind(),
        );
        domain
    }

    pub(super) fn logical_mut(&mut self) -> &mut LogicalInterfaces {
        &mut self.logical
    }

    pub(super) fn stack(&self) -> Arc<DomainStack> {
        self.stack.clone()
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn logical_membership_reservation_is_monotonic_and_failure_isolated() {
        let mut logical = LogicalInterfaces::new();
        assert_eq!(logical.members.len(), 1);
        let loopback = logical.loopback();
        assert_eq!(loopback.id().index(), 0);
        assert_eq!(loopback.ifindex(), 1);
        assert_eq!(loopback.name(), "lo");
        assert_eq!(loopback.kind(), LogicalInterfaceKind::Loopback);
        assert_eq!(loopback.netdev(), None);

        let first = logical.reserve_external(NetdevId::for_kunit(41));
        assert_eq!(logical.members.len(), 1);
        let first = logical.commit(first);
        assert_eq!(first.id().index(), 1);
        assert_eq!(first.ifindex(), 2);
        assert_eq!(first.name(), "eth0");
        assert_eq!(first.kind(), LogicalInterfaceKind::External);
        assert_eq!(first.netdev(), Some(NetdevId::for_kunit(41)));

        let failed = logical.reserve_external(NetdevId::for_kunit(42));
        logical.abort(failed);
        assert_eq!(logical.members, [logical.loopback().clone(), first.clone()]);

        let later = logical.reserve_external(NetdevId::for_kunit(43));
        let later = logical.commit(later);
        assert_eq!(later.id().index(), 3);
        assert_eq!(later.ifindex(), 4);
        assert_eq!(later.name(), "eth2");
        assert_eq!(later.netdev(), Some(NetdevId::for_kunit(43)));
        assert_ne!(later.id().index(), later.netdev().unwrap().index());
        assert_eq!(logical.members.len(), 3);
    }
}
