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
    /// Stable facts captured at publication. Link state may become stale and
    /// is never consulted by the runtime pump, whose provider remains the
    /// owner of current link and frame-resource truth.
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
/// The snapshot is an immutable copy of the registry's publication record.
/// Identity stays stable in the boot-only lifecycle, while observed facts such
/// as link state may become stale. The provider remains opaque and is moved
/// exactly once to the later attach authority.
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
    fn publication_identity_facts_and_failures_remain_record_local() {
        fn ready(
            origin: &str,
            ethernet_address: [u8; 6],
            max_frame_len: usize,
            link_state: LinkState,
            provider: usize,
        ) -> ReadyNetdev<usize> {
            ReadyNetdev::new(
                AnyIdentity::try_from(origin).unwrap(),
                Some(EthernetAddress::new(ethernet_address)),
                FrameCapabilities { max_frame_len },
                link_state,
                provider,
            )
        }

        let mut registry = Registry::new();
        let first_facts = InterfaceFacts {
            ethernet_address: Some(EthernetAddress::new([0x02, 0, 0, 0, 0, 1])),
            max_frame_len: 128,
            link_state: LinkState::Up,
        };
        let first = match registry.publish(ready(
            "virtio0",
            [0x02, 0, 0, 0, 0, 1],
            128,
            LinkState::Up,
            11,
        )) {
            Ok(published) => published,
            Err(_) => panic!("first publication must succeed"),
        };
        assert_eq!(first.snapshot().id().index(), 0);
        assert_eq!(first.snapshot().ifindex(), 1);
        assert_eq!(first.snapshot().name(), "eth0");
        assert_eq!(first.snapshot().origin(), "virtio0");
        assert_eq!(first.snapshot().facts(), first_facts);

        let duplicate = match registry.publish(ready(
            "virtio0",
            [0x02, 0, 0, 0, 0, 9],
            512,
            LinkState::Down,
            99,
        )) {
            Ok(_) => panic!("duplicate origin must be rejected"),
            Err(duplicate) => duplicate,
        };
        assert_eq!(duplicate.0, PublishError::DuplicateOrigin);
        assert_eq!(duplicate.1.provider, 99);
        assert_eq!(registry.records, [first.snapshot().clone()]);

        let second = match registry.publish(ready(
            "virtio1",
            [0x02, 0, 0, 0, 0, 2],
            256,
            LinkState::Unknown,
            22,
        )) {
            Ok(published) => published,
            Err(_) => panic!("second unique publication must succeed"),
        };
        assert_eq!(second.snapshot().id().index(), 1);
        assert_eq!(second.snapshot().ifindex(), 2);
        assert_eq!(second.snapshot().name(), "eth1");
        assert_eq!(second.snapshot().origin(), "virtio1");
        assert_eq!(
            second.snapshot().facts(),
            InterfaceFacts {
                ethernet_address: Some(EthernetAddress::new([0x02, 0, 0, 0, 0, 2])),
                max_frame_len: 256,
                link_state: LinkState::Unknown,
            }
        );
        assert_eq!(first.snapshot().facts(), first_facts);
        assert_eq!(registry.records.len(), 2);

        let (first_snapshot, first_provider) = first.into_parts();
        let (second_snapshot, second_provider) = second.into_parts();
        assert_eq!(first_provider, 11);
        assert_eq!(second_provider, 22);
        assert_ne!(first_snapshot.id(), second_snapshot.id());
    }
}
