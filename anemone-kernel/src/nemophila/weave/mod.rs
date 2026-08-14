//! Typed subsystem points and the runtime-owned weave protocol.

mod binding;
mod catalog;
mod host;

use core::marker::PhantomData;

use nemophila_wasm::WasmParams;

pub(super) use binding::CallbackBinding;
#[cfg(feature = "kunit")]
pub(super) use catalog::CatalogFailure;
// Stage 5's task-owned production declaration will consume this same path;
// Stage 4 ordinary builds intentionally have no production descriptor yet.
#[allow(unused_imports)]
pub(crate) use catalog::ProviderDescriptor;
pub(super) use catalog::{ProviderCatalog, provider_catalog};
pub(super) use host::WeaveFailure;

/// Point-owned cardinality consumed by the Nemophila runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BindingPolicy {
    Exclusive,
    Fanout,
}

impl BindingPolicy {
    const EXCLUSIVE: u8 = 1;
    const FANOUT: u8 = 2;

    const fn encode(self) -> u8 {
        match self {
            Self::Exclusive => Self::EXCLUSIVE,
            Self::Fanout => Self::FANOUT,
        }
    }

    fn decode(raw: u8) -> Option<Self> {
        match raw {
            Self::EXCLUSIVE => Some(Self::Exclusive),
            Self::FANOUT => Some(Self::Fanout),
            _ => None,
        }
    }
}

/// Stable kernel-internal identity of one extension point.
///
/// The numeric value, rather than linker position or descriptor address, is
/// the catalog identity. It is not a module-facing tag or management ABI.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct PointIdentity(u64);

impl PointIdentity {
    pub(crate) const fn new(raw: u64) -> Self {
        assert!(raw != 0, "Nemophila point identity zero is reserved");
        Self(raw)
    }

    const fn get(self) -> u64 {
        self.0
    }
}

/// One subsystem-owned observer point consumed by the weave mechanism.
///
/// WIT owns the module-visible names and value shape represented here. The
/// implementing subsystem owns the point identity, policy, native context and
/// the projection into that WIT shape. Runtime lifecycle is deliberately not
/// expressible through this interface.
pub(crate) trait PointSpec: 'static {
    type Context: 'static;
    type Params: WasmParams + 'static;

    const ID: PointIdentity;
    const POLICY: BindingPolicy;
    const REGISTRATION_MODULE: &'static str;
    const REGISTRATION_FUNCTION: &'static str;
    const CALLBACK_EXPORT: &'static str;

    fn lower(context: &Self::Context) -> Self::Params;
}

/// Typed invocation capability retained by a point's subsystem owner.
pub(crate) struct Point<P: PointSpec> {
    _point: PhantomData<fn() -> P>,
}

impl<P: PointSpec> Point<P> {
    pub(crate) const fn new() -> Self {
        Self {
            _point: PhantomData,
        }
    }

    /// Notify every currently admitted observer without exposing runtime state
    /// or module failures to the subsystem's business result.
    pub(crate) fn notify(&self, context: P::Context) {
        super::RUNTIME.invoke::<P>(&context);
    }
}

pub(super) fn install_point<P: PointSpec>(
    linker: &mut nemophila_wasm::Linker<super::host::HostContext>,
) -> Result<(), nemophila_wasm::Error> {
    host::install::<P>(linker)
}

macro_rules! declare_provider {
    ($visibility:vis static $point:ident: $spec:ty, $descriptor:ident) => {
        $visibility static $point: $crate::nemophila::weave::Point<$spec> =
            $crate::nemophila::weave::Point::new();
        #[used]
        #[unsafe(link_section = ".nemophila_providers")]
        static $descriptor: $crate::nemophila::weave::ProviderDescriptor =
            $crate::nemophila::weave::ProviderDescriptor::of::<$spec>();
    };
}

// Stage 5 activates the first production consumer. The KUnit-only task
// declaration already proves this sibling-subsystem macro path.
#[allow(unused_imports)]
pub(crate) use declare_provider;
