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
    pub(crate) fn from_parts(snapshot: NetdevSnapshot, provider: P) -> Self {
        Self { snapshot, provider }
    }

    pub(crate) fn snapshot(&self) -> &NetdevSnapshot {
        &self.snapshot
    }

    pub(crate) fn into_parts(self) -> (NetdevSnapshot, P) {
        (self.snapshot, self.provider)
    }
}

/// Concrete providers implement the one-shot attach operation outside the
/// registry owner. Failure must return the same published capability so the
/// registry can retain the durable published/unattached state without learning
/// provider or protocol policy.
pub(crate) trait PendingAttachProvider: Send + 'static {
    fn attach(published: PublishedNetdev<Self>) -> Result<(), PublishedNetdev<Self>>
    where
        Self: Sized;
}

trait ErasedPendingNetdev: Send {
    fn snapshot(&self) -> &NetdevSnapshot;

    fn attach(self: Box<Self>) -> Result<(), Box<dyn ErasedPendingNetdev>>;
}

impl<P: PendingAttachProvider> ErasedPendingNetdev for PublishedNetdev<P> {
    fn snapshot(&self) -> &NetdevSnapshot {
        self.snapshot()
    }

    fn attach(self: Box<Self>) -> Result<(), Box<dyn ErasedPendingNetdev>> {
        match P::attach(*self) {
            Ok(()) => Ok(()),
            Err(published) => Err(Box::new(published)),
        }
    }
}

/// Registry-owned, one-shot pending handoff.
///
/// Type erasure ends at `attach`: the concrete implementation immediately
/// re-enters the generic worker preparation path. No frame token, queue fact,
/// provider identity, or protocol object crosses this boundary.
pub(crate) struct PendingNetdev {
    inner: Box<dyn ErasedPendingNetdev>,
}

impl PendingNetdev {
    fn new<P: PendingAttachProvider>(published: PublishedNetdev<P>) -> Self {
        Self {
            inner: Box::new(published),
        }
    }

    pub(crate) fn snapshot(&self) -> &NetdevSnapshot {
        self.inner.snapshot()
    }

    pub(crate) fn attach(self) -> Result<(), Self> {
        match self.inner.attach() {
            Ok(()) => Ok(()),
            Err(inner) => Err(Self { inner }),
        }
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
    pending: Vec<PendingNetdev>,
}

impl Registry {
    const fn new() -> Self {
        Self {
            next_id: 0,
            records: Vec::new(),
            pending: Vec::new(),
        }
    }

    fn publish<P: PendingAttachProvider>(
        &mut self,
        ready: ReadyNetdev<P>,
    ) -> Result<NetdevSnapshot, (PublishError, ReadyNetdev<P>)> {
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
        let pending = PendingNetdev::new(PublishedNetdev::from_parts(
            snapshot.clone(),
            ready.provider,
        ));
        // The record and its attach capability are committed under the same
        // registry lock. Normal publication failures above leave neither half.
        self.records.push(snapshot.clone());
        self.pending.push(pending);

        Ok(snapshot)
    }

    fn take_pending(&mut self) -> Vec<PendingNetdev> {
        core::mem::take(&mut self.pending)
    }

    fn retain_pending(&mut self, pending: PendingNetdev) {
        let id = pending.snapshot().id();
        assert!(
            self.records.iter().any(|record| record.id() == id),
            "pending network capability lost its publication record"
        );
        assert!(
            self.pending
                .iter()
                .all(|existing| existing.snapshot().id() != id),
            "pending network capability retained twice"
        );
        self.pending.push(pending);
    }
}

static REGISTRY: Lazy<SpinLock<Registry>> = Lazy::new(|| SpinLock::new(Registry::new()));

pub(crate) fn publish<P: PendingAttachProvider>(
    ready: ReadyNetdev<P>,
) -> Result<NetdevSnapshot, (PublishError, ReadyNetdev<P>)> {
    REGISTRY.lock_irqsave().publish(ready)
}

pub(crate) fn take_pending() -> Vec<PendingNetdev> {
    REGISTRY.lock_irqsave().take_pending()
}

pub(crate) fn retain_pending(pending: PendingNetdev) {
    REGISTRY.lock_irqsave().retain_pending(pending);
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    struct AttachedProvider(usize);

    impl PendingAttachProvider for AttachedProvider {
        fn attach(published: PublishedNetdev<Self>) -> Result<(), PublishedNetdev<Self>> {
            assert_eq!(published.provider.0, 11);
            Ok(())
        }
    }

    struct RetainedProvider(usize);

    impl PendingAttachProvider for RetainedProvider {
        fn attach(published: PublishedNetdev<Self>) -> Result<(), PublishedNetdev<Self>> {
            assert_eq!(published.provider.0, 22);
            Err(published)
        }
    }

    #[kunit]
    fn publication_identity_facts_and_failures_remain_record_local() {
        fn ready<P>(
            origin: &str,
            ethernet_address: [u8; 6],
            max_frame_len: usize,
            link_state: LinkState,
            provider: P,
        ) -> ReadyNetdev<P> {
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
            AttachedProvider(11),
        )) {
            Ok(published) => published,
            Err(_) => panic!("first publication must succeed"),
        };
        assert_eq!(first.id().index(), 0);
        assert_eq!(first.ifindex(), 1);
        assert_eq!(first.name(), "eth0");
        assert_eq!(first.origin(), "virtio0");
        assert_eq!(first.facts(), first_facts);

        let duplicate = match registry.publish(ready(
            "virtio0",
            [0x02, 0, 0, 0, 0, 9],
            512,
            LinkState::Down,
            AttachedProvider(99),
        )) {
            Ok(_) => panic!("duplicate origin must be rejected"),
            Err(duplicate) => duplicate,
        };
        assert_eq!(duplicate.0, PublishError::DuplicateOrigin);
        assert_eq!(duplicate.1.provider.0, 99);
        assert_eq!(registry.records, [first.clone()]);

        let second = match registry.publish(ready(
            "virtio1",
            [0x02, 0, 0, 0, 0, 2],
            256,
            LinkState::Unknown,
            RetainedProvider(22),
        )) {
            Ok(published) => published,
            Err(_) => panic!("second unique publication must succeed"),
        };
        assert_eq!(second.id().index(), 1);
        assert_eq!(second.ifindex(), 2);
        assert_eq!(second.name(), "eth1");
        assert_eq!(second.origin(), "virtio1");
        assert_eq!(
            second.facts(),
            InterfaceFacts {
                ethernet_address: Some(EthernetAddress::new([0x02, 0, 0, 0, 0, 2])),
                max_frame_len: 256,
                link_state: LinkState::Unknown,
            }
        );
        assert_eq!(first.facts(), first_facts);
        assert_eq!(registry.records.len(), 2);
        assert_ne!(first.id(), second.id());

        let pending = registry.take_pending();
        assert_eq!(pending.len(), 2);
        for capability in pending {
            if let Err(capability) = capability.attach() {
                registry.retain_pending(capability);
            }
        }
        assert_eq!(registry.pending.len(), 1);
        assert_eq!(registry.pending[0].snapshot(), &second);
        assert_eq!(registry.records, [first, second]);
    }
}
