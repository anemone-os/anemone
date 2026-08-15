use crate::arch::link_symbols::{__enemophila_providers, __snemophila_providers};

use super::{BindingPolicy, PointIdentity, PointSpec};

/// Immutable point facts emitted by a subsystem-owned provider declaration.
///
/// The descriptor only determines compiled-in provider availability and
/// policy. Dynamic callback, reservation and lifecycle state remain entirely
/// runtime-owned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C, align(8))]
pub(crate) struct ProviderDescriptor {
    point: u64,
    policy: u8,
    reserved: [u8; 7],
}

impl ProviderDescriptor {
    pub(crate) const fn of<P: PointSpec>() -> Self {
        Self {
            point: P::ID.get(),
            policy: P::POLICY.encode(),
            reserved: [0; 7],
        }
    }

    #[cfg(feature = "kunit")]
    pub(in crate::nemophila) const fn invalid<P: PointSpec>() -> Self {
        Self {
            point: P::ID.get(),
            policy: P::POLICY.encode(),
            reserved: [1; 7],
        }
    }

    fn resolve(&self) -> Result<ResolvedProvider, CatalogFailure> {
        if self.point == 0 || self.reserved != [0; 7] {
            return Err(CatalogFailure::InvalidDescriptor);
        }
        let Some(policy) = BindingPolicy::decode(self.policy) else {
            return Err(CatalogFailure::InvalidDescriptor);
        };
        Ok(ResolvedProvider {
            point: PointIdentity::new(self.point),
            policy,
        })
    }
}

const _: () = assert!(core::mem::size_of::<ProviderDescriptor>() == 16);
const _: () = assert!(core::mem::align_of::<ProviderDescriptor>() == 8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::nemophila) enum CatalogFailure {
    InvalidDescriptor,
    DuplicatePoint,
}

#[derive(Clone, Copy, Debug)]
struct ResolvedProvider {
    point: PointIdentity,
    policy: BindingPolicy,
}

/// Validated immutable view of compiled-in provider descriptors.
#[derive(Clone, Copy)]
pub(in crate::nemophila) struct ProviderCatalog {
    descriptors: &'static [ProviderDescriptor],
}

impl ProviderCatalog {
    pub(in crate::nemophila) fn validate(
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

    pub(in crate::nemophila) fn policy(&self, point: PointIdentity) -> Option<BindingPolicy> {
        self.descriptors.iter().find_map(|descriptor| {
            let resolved = descriptor
                .resolve()
                .expect("validated Nemophila provider catalog changed");
            (resolved.point == point).then_some(resolved.policy)
        })
    }
}

pub(in crate::nemophila) fn provider_catalog() -> ProviderCatalog {
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
        // Safety: the linker symbols bound a retained read-only section whose
        // entries are emitted only by `declare_provider!`.
        core::slice::from_raw_parts(
            start as *const ProviderDescriptor,
            (end - start) / core::mem::size_of::<ProviderDescriptor>(),
        )
    }
}
