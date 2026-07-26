//! Boot-time network-device identity and publication.

use anemone_net_api::{EthernetAddress, FrameCapabilities, InterfaceFacts, LinkState};

use crate::{
    prelude::*,
    utils::identity::{AnyIdentity, GeneralIdentity},
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct NetdevId(u32);

impl NetdevId {
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// Immutable facts committed by the network-device registry at publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetdevSnapshot {
    id: NetdevId,
    ifindex: u32,
    name: GeneralIdentity,
    origin: AnyIdentity,
    facts: InterfaceFacts,
}

impl NetdevSnapshot {
    pub const fn id(&self) -> NetdevId {
        self.id
    }

    pub const fn ifindex(&self) -> u32 {
        self.ifindex
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn origin(&self) -> &str {
        self.origin.as_str()
    }

    pub const fn facts(&self) -> InterfaceFacts {
        self.facts
    }
}

/// A provider capability whose owner-local initialization is already complete.
///
/// The constructor is crate-private so concrete drivers can only publish the
/// ready capability their probe path explicitly produces. The registry never
/// sees or stores the provider's backing, queue, or transport representation.
pub(crate) struct ReadyNetdev<P> {
    origin: AnyIdentity,
    ethernet_address: Option<EthernetAddress>,
    frame_capabilities: FrameCapabilities,
    link_state: LinkState,
    provider: P,
}

impl<P> ReadyNetdev<P> {
    pub(crate) fn new(
        origin: AnyIdentity,
        ethernet_address: Option<EthernetAddress>,
        frame_capabilities: FrameCapabilities,
        link_state: LinkState,
        provider: P,
    ) -> Self {
        Self {
            origin,
            ethernet_address,
            frame_capabilities,
            link_state,
            provider,
        }
    }
}

/// Typed publication capability minted by the registry.
///
/// The snapshot is an immutable copy of the registry's publication record, so
/// it cannot become stale in the boot-only R0 lifecycle. The provider remains
/// opaque and is moved exactly once to the later attach authority.
pub(crate) struct PublishedNetdev<P> {
    snapshot: NetdevSnapshot,
    provider: P,
}

impl<P> PublishedNetdev<P> {
    pub(crate) fn snapshot(&self) -> &NetdevSnapshot {
        &self.snapshot
    }

    pub(crate) fn into_parts(self) -> (NetdevSnapshot, P) {
        (self.snapshot, self.provider)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PublishError {
    DuplicateOrigin,
    IdentityExhausted,
    NameTooLong,
}

struct Registry {
    next_id: u32,
    records: Vec<NetdevSnapshot>,
}

impl Registry {
    const fn new() -> Self {
        Self {
            next_id: 0,
            records: Vec::new(),
        }
    }

    fn publish<P>(
        &mut self,
        ready: ReadyNetdev<P>,
    ) -> Result<PublishedNetdev<P>, (PublishError, ReadyNetdev<P>)> {
        if self
            .records
            .iter()
            .any(|record| record.origin == ready.origin)
        {
            return Err((PublishError::DuplicateOrigin, ready));
        }

        let raw_id = self.next_id;
        let Some(ifindex) = raw_id.checked_add(1) else {
            return Err((PublishError::IdentityExhausted, ready));
        };
        let name = match GeneralIdentity::try_from_fmt(format_args!("eth{raw_id}")) {
            Ok(name) => name,
            Err(_) => return Err((PublishError::NameTooLong, ready)),
        };
        self.next_id = ifindex;

        let snapshot = NetdevSnapshot {
            id: NetdevId(raw_id),
            ifindex,
            name,
            origin: ready.origin,
            facts: InterfaceFacts {
                ethernet_address: ready.ethernet_address,
                max_frame_len: ready.frame_capabilities.max_frame_len,
                link_state: ready.link_state,
            },
        };
        self.records.push(snapshot.clone());

        Ok(PublishedNetdev {
            snapshot,
            provider: ready.provider,
        })
    }
}

static REGISTRY: Lazy<SpinLock<Registry>> = Lazy::new(|| SpinLock::new(Registry::new()));

pub(crate) fn publish<P>(
    ready: ReadyNetdev<P>,
) -> Result<PublishedNetdev<P>, (PublishError, ReadyNetdev<P>)> {
    REGISTRY.lock_irqsave().publish(ready)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn identity_and_name_are_monotonic_and_duplicate_publication_is_rejected() {
        fn ready(origin: &str) -> ReadyNetdev<()> {
            ReadyNetdev::new(
                AnyIdentity::try_from(origin).unwrap(),
                None,
                FrameCapabilities { max_frame_len: 128 },
                LinkState::Unknown,
                (),
            )
        }

        let mut registry = Registry::new();
        let first = match registry.publish(ready("virtio0")) {
            Ok(published) => published,
            Err(_) => panic!("first publication must succeed"),
        };
        assert_eq!(first.snapshot().id().index(), 0);
        assert_eq!(first.snapshot().ifindex(), 1);
        assert_eq!(first.snapshot().name(), "eth0");
        assert!(matches!(
            registry.publish(ready("virtio0")),
            Err((PublishError::DuplicateOrigin, _))
        ));

        let second = match registry.publish(ready("virtio1")) {
            Ok(published) => published,
            Err(_) => panic!("second unique publication must succeed"),
        };
        assert_eq!(second.snapshot().id().index(), 1);
        assert_eq!(second.snapshot().ifindex(), 2);
        assert_eq!(second.snapshot().name(), "eth1");
    }
}
