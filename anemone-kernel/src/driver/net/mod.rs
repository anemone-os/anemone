mod virtio;

#[cfg(feature = "kunit")]
pub(crate) use virtio::stage1_probe_stats;
pub(crate) use virtio::take_published_netdevs;
