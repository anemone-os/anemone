//! Boot-only RTC provider core.
//!
//! Concrete drivers publish a device-owned read capability here. This module
//! owns origin admission, selection, sealing, and the single boot read; it
//! never exposes a runtime RTC handle or participates in clock projection.

use crate::{device::discovery::fwnode::FwNode, prelude::*, time::RealtimeInstant};

pub(crate) trait RtcProvider: Send + Sync {
    fn read_time(&self) -> Result<RealtimeInstant, RtcReadError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RtcReadError {
    DeviceIo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegisterError {
    DuplicateOrigin,
    Finalized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionReason {
    MachinePreference,
    SingleProvider,
}

struct ProviderEntry {
    origin: Arc<dyn FwNode>,
    provider: Arc<dyn RtcProvider>,
}

enum Phase {
    Open(Vec<ProviderEntry>),
    Finalized,
}

struct Registry {
    phase: Phase,
}

impl Registry {
    const fn new() -> Self {
        Self {
            phase: Phase::Open(Vec::new()),
        }
    }

    fn register(
        &mut self,
        origin: Arc<dyn FwNode>,
        provider: Arc<dyn RtcProvider>,
    ) -> Result<(), RegisterError> {
        let Phase::Open(entries) = &mut self.phase else {
            return Err(RegisterError::Finalized);
        };
        if entries
            .iter()
            .any(|entry| entry.origin.equals(origin.as_ref()))
        {
            return Err(RegisterError::DuplicateOrigin);
        }
        entries.push(ProviderEntry { origin, provider });
        Ok(())
    }

    fn finalize(&mut self, preferred: Option<&dyn FwNode>) -> Decision {
        let Phase::Open(entries) = core::mem::replace(&mut self.phase, Phase::Finalized) else {
            return Decision::AlreadyFinalized;
        };
        if let Some(preferred) = preferred {
            return entries
                .into_iter()
                .find(|entry| entry.origin.equals(preferred))
                .map(|entry| Decision::Selected {
                    entry,
                    reason: SelectionReason::MachinePreference,
                })
                .unwrap_or(Decision::PreferenceMiss);
        }
        match entries.len() {
            0 => Decision::NoProvider,
            1 => Decision::Selected {
                entry: entries.into_iter().next().unwrap(),
                reason: SelectionReason::SingleProvider,
            },
            count => Decision::Ambiguous { count },
        }
    }
}

enum Decision {
    Selected {
        entry: ProviderEntry,
        reason: SelectionReason,
    },
    NoProvider,
    PreferenceMiss,
    Ambiguous {
        count: usize,
    },
    AlreadyFinalized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Seed(RealtimeInstant),
    NoProvider,
    PreferenceMiss,
    Ambiguous { count: usize },
    ReadFailed(RtcReadError),
    AlreadyFinalized,
}

static REGISTRY: Lazy<SpinLock<Registry>> = Lazy::new(|| SpinLock::new(Registry::new()));

fn origin_label(origin: &dyn FwNode) -> String {
    origin
        .as_of_node()
        .map(|node| node.node().path())
        .unwrap_or_else(|| String::from("<non-OF firmware node>"))
}

fn finish_decision(decision: Decision) -> Outcome {
    match decision {
        Decision::Selected { entry, reason } => {
            let label = origin_label(entry.origin.as_ref());
            kinfoln!(
                "RTC provider selected: origin={} reason={:?}",
                label,
                reason
            );
            match entry.provider.read_time() {
                Ok(epoch) => {
                    kinfoln!(
                        "RTC provider read succeeded: origin={} epoch_ns={}",
                        label,
                        epoch.as_nanos()
                    );
                    Outcome::Seed(epoch)
                },
                Err(error) => {
                    kwarningln!(
                        "RTC provider read failed: origin={} reason={:?}; no fallback",
                        label,
                        error
                    );
                    Outcome::ReadFailed(error)
                },
            }
        },
        Decision::NoProvider => {
            kinfoln!("RTC boot selection: no provider; retaining zero offset");
            Outcome::NoProvider
        },
        Decision::PreferenceMiss => {
            kwarningln!("RTC boot selection: machine preference did not match; no fallback");
            Outcome::PreferenceMiss
        },
        Decision::Ambiguous { count } => {
            kwarningln!(
                "RTC boot selection: {} providers without machine preference; no provider selected",
                count
            );
            Outcome::Ambiguous { count }
        },
        Decision::AlreadyFinalized => {
            kwarningln!("RTC boot selection was already finalized");
            Outcome::AlreadyFinalized
        },
    }
}

fn finish(registry: &mut Registry, preferred: Option<&dyn FwNode>) -> Outcome {
    finish_decision(registry.finalize(preferred))
}

pub(crate) fn register_provider(
    origin: Arc<dyn FwNode>,
    provider: Arc<dyn RtcProvider>,
) -> Result<(), RegisterError> {
    let label = origin_label(origin.as_ref());
    REGISTRY.lock_irqsave().register(origin, provider)?;
    kinfoln!("RTC provider registered: origin={}", label);
    Ok(())
}

/// Seal registration, select at most one provider, and read it once.
pub(crate) fn finalize_boot(preferred: Option<Arc<dyn FwNode>>) -> Option<RealtimeInstant> {
    // Seal and detach the chosen capability under the registry lock, then
    // perform the fallible device read without holding subsystem state.
    let decision = REGISTRY.lock_irqsave().finalize(preferred.as_deref());
    match finish_decision(decision) {
        Outcome::Seed(epoch) => Some(epoch),
        Outcome::NoProvider
        | Outcome::PreferenceMiss
        | Outcome::Ambiguous { .. }
        | Outcome::ReadFailed(_)
        | Outcome::AlreadyFinalized => None,
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use core::any::Any;

    use super::*;
    use crate::device::discovery::fwnode::StdoutConfig;

    #[derive(Debug)]
    struct Origin(u32);

    impl FwNode for Origin {
        fn equals(&self, other: &dyn FwNode) -> bool {
            (other as &dyn Any)
                .downcast_ref::<Self>()
                .is_some_and(|other| self.0 == other.0)
        }
        fn prop_read_u32(&self, _: &str) -> Option<u32> {
            None
        }
        fn prop_read_u64(&self, _: &str) -> Option<u64> {
            None
        }
        fn prop_read_str(&self, _: &str) -> Option<String> {
            None
        }
        fn prop_read_present(&self, _: &str) -> bool {
            false
        }
        fn prop_read_raw(&self, _: &str) -> Option<&[u8]> {
            None
        }
        fn interrupt_parent(&self) -> Option<Arc<dyn FwNode>> {
            None
        }
        fn interrupt_info(&self) -> Option<&[u8]> {
            None
        }
        fn stdout_config(&self) -> Option<StdoutConfig<'_>> {
            None
        }
    }

    struct Provider {
        result: Result<RealtimeInstant, RtcReadError>,
        reads: Arc<AtomicUsize>,
    }

    impl Provider {
        fn new(result: Result<RealtimeInstant, RtcReadError>) -> (Arc<Self>, Arc<AtomicUsize>) {
            let reads = Arc::new(AtomicUsize::new(0));
            (
                Arc::new(Self {
                    result,
                    reads: reads.clone(),
                }),
                reads,
            )
        }
    }

    impl RtcProvider for Provider {
        fn read_time(&self) -> Result<RealtimeInstant, RtcReadError> {
            self.reads.fetch_add(1, Ordering::AcqRel);
            self.result
        }
    }

    fn origin(id: u32) -> Arc<dyn FwNode> {
        Arc::new(Origin(id))
    }

    #[kunit]
    fn registry_admits_unique_origins_and_seals_once() {
        let mut registry = Registry::new();
        let (first, reads) = Provider::new(Ok(RealtimeInstant::from_nanos(10)));
        let (duplicate, _) = Provider::new(Ok(RealtimeInstant::from_nanos(20)));
        assert_eq!(registry.register(origin(1), first), Ok(()));
        assert_eq!(
            registry.register(origin(1), duplicate),
            Err(RegisterError::DuplicateOrigin)
        );
        assert_eq!(
            finish(&mut registry, None),
            Outcome::Seed(RealtimeInstant::from_nanos(10))
        );
        assert_eq!(reads.load(Ordering::Acquire), 1);
        let _ = crate::time::realtime_ns();
        let _ = crate::time::coarse_realtime_ns();
        assert_eq!(reads.load(Ordering::Acquire), 1);
        let (late, _) = Provider::new(Ok(RealtimeInstant::from_nanos(30)));
        assert_eq!(
            registry.register(origin(2), late),
            Err(RegisterError::Finalized)
        );
        assert_eq!(finish(&mut registry, None), Outcome::AlreadyFinalized);
    }

    #[kunit]
    fn selection_preference_and_ambiguity_are_deterministic() {
        let mut registry = Registry::new();
        let (first, first_reads) = Provider::new(Ok(RealtimeInstant::from_nanos(10)));
        let (second, second_reads) = Provider::new(Ok(RealtimeInstant::from_nanos(20)));
        registry.register(origin(1), first).unwrap();
        registry.register(origin(2), second).unwrap();
        let preferred = origin(2);
        assert_eq!(
            finish(&mut registry, Some(preferred.as_ref())),
            Outcome::Seed(RealtimeInstant::from_nanos(20))
        );
        assert_eq!(first_reads.load(Ordering::Acquire), 0);
        assert_eq!(second_reads.load(Ordering::Acquire), 1);

        let mut ambiguous = Registry::new();
        let (first, first_reads) = Provider::new(Ok(RealtimeInstant::from_nanos(30)));
        let (second, second_reads) = Provider::new(Ok(RealtimeInstant::from_nanos(40)));
        ambiguous.register(origin(1), first).unwrap();
        ambiguous.register(origin(2), second).unwrap();
        assert_eq!(
            finish(&mut ambiguous, None),
            Outcome::Ambiguous { count: 2 }
        );
        assert_eq!(first_reads.load(Ordering::Acquire), 0);
        assert_eq!(second_reads.load(Ordering::Acquire), 0);
    }

    #[kunit]
    fn explicit_failure_does_not_fallback_or_repeat_and_clock_reads_do_not_reenter() {
        let mut registry = Registry::new();
        let (failed, failed_reads) = Provider::new(Err(RtcReadError::DeviceIo));
        let (other, other_reads) = Provider::new(Ok(RealtimeInstant::from_nanos(50)));
        registry.register(origin(1), failed).unwrap();
        registry.register(origin(2), other).unwrap();
        let preferred = origin(1);
        assert_eq!(
            finish(&mut registry, Some(preferred.as_ref())),
            Outcome::ReadFailed(RtcReadError::DeviceIo)
        );
        assert_eq!(failed_reads.load(Ordering::Acquire), 1);
        assert_eq!(other_reads.load(Ordering::Acquire), 0);
        assert_eq!(
            finish(&mut registry, Some(preferred.as_ref())),
            Outcome::AlreadyFinalized
        );
    }

    #[kunit]
    fn preference_miss_and_empty_registry_keep_zero_fallback() {
        let mut empty = Registry::new();
        assert_eq!(finish(&mut empty, None), Outcome::NoProvider);
        let mut missed = Registry::new();
        let (provider, reads) = Provider::new(Ok(RealtimeInstant::from_nanos(60)));
        missed.register(origin(1), provider).unwrap();
        let absent = origin(9);
        assert_eq!(
            finish(&mut missed, Some(absent.as_ref())),
            Outcome::PreferenceMiss
        );
        assert_eq!(reads.load(Ordering::Acquire), 0);
    }
}
