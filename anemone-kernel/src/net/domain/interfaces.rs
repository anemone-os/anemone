//! Initial-domain logical-interface membership and identity owner.

use crate::{device::net::NetdevId, prelude::*, utils::identity::GeneralIdentity};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub(in crate::net) struct LogicalInterfaceId(u32);

impl LogicalInterfaceId {
    pub(in crate::net) const fn index(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::net) enum LogicalInterfaceKind {
    Loopback,
    External,
}

/// Immutable domain-membership fact.
///
/// `netdev` is an opaque association for external-interface diagnosis; it is
/// never used to derive the logical or protocol identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::net) struct LogicalInterfaceSnapshot {
    id: LogicalInterfaceId,
    ifindex: u32,
    name: GeneralIdentity,
    kind: LogicalInterfaceKind,
    netdev: Option<NetdevId>,
}

impl LogicalInterfaceSnapshot {
    pub(in crate::net) const fn id(&self) -> LogicalInterfaceId {
        self.id
    }

    pub(in crate::net) const fn ifindex(&self) -> u32 {
        self.ifindex
    }

    pub(in crate::net) fn name(&self) -> &str {
        self.name.as_str()
    }

    pub(in crate::net) const fn kind(&self) -> LogicalInterfaceKind {
        self.kind
    }

    pub(in crate::net) const fn netdev(&self) -> Option<NetdevId> {
        self.netdev
    }
}

pub(in crate::net) struct LogicalInterfaceReservation {
    snapshot: LogicalInterfaceSnapshot,
    finished: bool,
}

impl LogicalInterfaceReservation {
    pub(in crate::net) fn snapshot(&self) -> &LogicalInterfaceSnapshot {
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
pub(in crate::net) struct LogicalInterfaces {
    members: Vec<LogicalInterfaceSnapshot>,
    next_id: u32,
    next_ifindex: u32,
    next_external_ordinal: u32,
}

impl LogicalInterfaces {
    pub(super) fn new() -> Self {
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

    pub(in crate::net) fn reserve_external(
        &mut self,
        netdev: NetdevId,
    ) -> LogicalInterfaceReservation {
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

    pub(in crate::net) fn commit(
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

    pub(in crate::net) fn abort(&mut self, mut reservation: LogicalInterfaceReservation) {
        assert!(
            self.members
                .iter()
                .all(|member| member.id != reservation.snapshot.id),
            "published logical interface cannot be aborted"
        );
        reservation.finished = true;
    }

    pub(super) fn loopback(&self) -> &LogicalInterfaceSnapshot {
        let loopback = &self.members[0];
        assert_eq!(loopback.kind, LogicalInterfaceKind::Loopback);
        loopback
    }
}

#[cfg(feature = "kunit")]
impl LogicalInterfaceSnapshot {
    pub(in crate::net) fn for_control_plane_kunit(name: &str, netdev: NetdevId) -> Self {
        Self {
            id: LogicalInterfaceId(u32::MAX),
            ifindex: u32::MAX,
            name: GeneralIdentity::try_from(name).expect("KUnit interface name must fit"),
            kind: LogicalInterfaceKind::External,
            netdev: Some(netdev),
        }
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
