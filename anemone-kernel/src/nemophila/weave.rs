use nemophila_wasm::TypedFunc;

use crate::arch::link_symbols::{__enemophila_providers, __snemophila_providers};

const CLONE_OBSERVER_POINT: u64 = 1;
#[cfg(feature = "kunit")]
const KUNIT_EXCLUSIVE_POINT: u64 = u64::MAX;
const CLONE_OBSERVER_CALLBACK: u8 = 1;
const POLICY_EXCLUSIVE: u8 = 1;
const POLICY_FANOUT: u8 = 2;

/// The point-owned cardinality policy consumed by the Nemophila runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BindingPolicy {
    Exclusive,
    Fanout,
}

/// Stable kernel-internal identity of one typed extension point.
///
/// The numeric value, rather than linker position or descriptor address, is
/// the catalog identity. It is not a module-facing tag or management ABI.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct PointIdentity(u64);

impl PointIdentity {
    pub(super) const CLONE_OBSERVER: Self = Self(CLONE_OBSERVER_POINT);

    #[cfg(feature = "kunit")]
    pub(super) const KUNIT_EXCLUSIVE: Self = Self(KUNIT_EXCLUSIVE_POINT);
}

/// The typed point capability retained by its subsystem owner.
///
/// Stage 4 Checkpoint 1 only establishes declaration and catalog resolution;
/// invocation is added with the complete lifecycle protocol in Checkpoint 2.
#[derive(Debug)]
pub(crate) struct CloneObserverPoint {
    _private: (),
}

impl CloneObserverPoint {
    pub(crate) const fn new() -> Self {
        Self { _private: () }
    }

    /// Invoke the typed point without exposing runtime or instance state to
    /// the declaring subsystem. Callback absence, return and trap are all
    /// observational and cannot alter the provider's business result.
    pub(crate) fn invoke(&self, creator_tid: u32, child_tid: u32) {
        super::invoke_clone_observers(creator_tid, child_tid);
    }
}

/// Immutable point facts emitted by the declaring subsystem.
///
/// Raw integer fields keep catalog validation well-defined even for malformed
/// section contents. The private constructors are the only ordinary source of
/// descriptors; invalid entries are kernel invariant failures, not module
/// registration results.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C, align(8))]
pub(crate) struct ProviderDescriptor {
    point: u64,
    policy: u8,
    callback: u8,
    reserved: [u8; 6],
}

impl ProviderDescriptor {
    pub(crate) const fn clone_observer() -> Self {
        Self {
            point: CLONE_OBSERVER_POINT,
            policy: POLICY_FANOUT,
            callback: CLONE_OBSERVER_CALLBACK,
            reserved: [0; 6],
        }
    }

    #[cfg(feature = "kunit")]
    pub(crate) const fn kunit_exclusive() -> Self {
        Self {
            point: KUNIT_EXCLUSIVE_POINT,
            policy: POLICY_EXCLUSIVE,
            callback: CLONE_OBSERVER_CALLBACK,
            reserved: [0; 6],
        }
    }

    #[cfg(feature = "kunit")]
    pub(super) const fn invalid() -> Self {
        Self {
            point: 0,
            policy: 0,
            callback: 0,
            reserved: [1; 6],
        }
    }

    fn resolve(&self) -> Result<ResolvedProvider, CatalogFailure> {
        if self.callback != CLONE_OBSERVER_CALLBACK || self.reserved != [0; 6] {
            return Err(CatalogFailure::InvalidDescriptor);
        }
        match (self.point, self.policy) {
            (CLONE_OBSERVER_POINT, POLICY_FANOUT) => Ok(ResolvedProvider {
                point: PointIdentity::CLONE_OBSERVER,
                policy: BindingPolicy::Fanout,
            }),
            #[cfg(feature = "kunit")]
            (KUNIT_EXCLUSIVE_POINT, POLICY_EXCLUSIVE) => Ok(ResolvedProvider {
                point: PointIdentity::KUNIT_EXCLUSIVE,
                policy: BindingPolicy::Exclusive,
            }),
            _ => Err(CatalogFailure::InvalidDescriptor),
        }
    }
}

const _: () = assert!(core::mem::size_of::<ProviderDescriptor>() == 16);
const _: () = assert!(core::mem::align_of::<ProviderDescriptor>() == 8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CatalogFailure {
    InvalidDescriptor,
    DuplicatePoint,
}

#[derive(Clone, Copy, Debug)]
struct ResolvedProvider {
    point: PointIdentity,
    policy: BindingPolicy,
}

/// A validated immutable view of the linker-owned provider descriptors.
#[derive(Clone, Copy)]
pub(super) struct ProviderCatalog {
    descriptors: &'static [ProviderDescriptor],
}

impl ProviderCatalog {
    pub(super) fn validate(
        descriptors: &'static [ProviderDescriptor],
    ) -> Result<Self, CatalogFailure> {
        for (index, descriptor) in descriptors.iter().enumerate() {
            let resolved = descriptor.resolve()?;
            for previous in &descriptors[..index] {
                if previous.resolve()?.point == resolved.point {
                    return Err(CatalogFailure::DuplicatePoint);
                }
            }
        }
        Ok(Self { descriptors })
    }

    pub(super) fn policy(&self, point: PointIdentity) -> Option<BindingPolicy> {
        self.descriptors.iter().find_map(|descriptor| {
            let resolved = descriptor
                .resolve()
                .expect("validated Nemophila provider catalog changed");
            (resolved.point == point).then_some(resolved.policy)
        })
    }
}

pub(super) fn provider_catalog() -> ProviderCatalog {
    ProviderCatalog::validate(linker_descriptors())
        .expect("invalid or duplicate Nemophila provider descriptor")
}

fn linker_descriptors() -> &'static [ProviderDescriptor] {
    unsafe {
        let start = __snemophila_providers as *const () as usize;
        let end = __enemophila_providers as *const () as usize;
        assert!(start.is_multiple_of(core::mem::align_of::<ProviderDescriptor>()));
        assert!(end >= start);
        assert!((end - start).is_multiple_of(core::mem::size_of::<ProviderDescriptor>()));
        // Safety: both linker symbols bound one read-only section populated
        // only by `ProviderDescriptor` statics emitted by the declaration
        // macros below. The section is retained for the kernel image lifetime.
        core::slice::from_raw_parts(
            start as *const ProviderDescriptor,
            (end - start) / core::mem::size_of::<ProviderDescriptor>(),
        )
    }
}

/// The callable captured by successful clone-observer registration.
///
/// This handle belongs to the same Store later owned by `RuntimeInstance`;
/// reservation and publication move it without copying callback state.
#[derive(Clone, Copy)]
pub(super) struct CallbackBinding {
    pub(super) callback: TypedFunc<(i32, i32), ()>,
}

impl CallbackBinding {
    pub(super) fn clone_observer(callback: TypedFunc<(i32, i32), ()>) -> Self {
        Self { callback }
    }
}

macro_rules! declare_clone_observer_provider {
    ($visibility:vis $point:ident, $descriptor:ident) => {
        $visibility static $point: $crate::nemophila::weave::CloneObserverPoint =
            $crate::nemophila::weave::CloneObserverPoint::new();
        #[used]
        #[unsafe(link_section = ".nemophila_providers")]
        static $descriptor: $crate::nemophila::weave::ProviderDescriptor =
            $crate::nemophila::weave::ProviderDescriptor::clone_observer();
    };
}

// Stage 5 point owners consume this declaration surface; Stage 4 ordinary
// builds intentionally have no production descriptor yet.
#[allow(unused_imports)]
pub(crate) use declare_clone_observer_provider;

#[cfg(feature = "kunit")]
macro_rules! declare_kunit_exclusive_provider {
    ($descriptor:ident) => {
        #[used]
        #[unsafe(link_section = ".nemophila_providers")]
        static $descriptor: $crate::nemophila::weave::ProviderDescriptor =
            $crate::nemophila::weave::ProviderDescriptor::kunit_exclusive();
    };
}

#[cfg(feature = "kunit")]
pub(super) use declare_kunit_exclusive_provider;
