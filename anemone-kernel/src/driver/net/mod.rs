mod virtio;

#[cfg(feature = "kunit")]
pub(crate) use virtio::stage2_conformance_stats;
pub(crate) use virtio::take_published_netdevs;
