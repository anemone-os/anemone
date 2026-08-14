//! Data structures specialized for usage in the Wasmi interpreter.
//!
//! The interpreter uses allocation-only B-tree collections so its production
//! dependency graph does not require a random hash seed or a second backend.
//!
//! # Provided Data Structures
//!
//! - [`Arena`]: typed arena for fast allocations and accesses
//! - [`DedupArena`]: typed arena that also deduplicates with [`BTreeMap`]
//! - [`ComponentVec`]: useful to add properties to entities stored in an
//!   [`Arena`] or [`DedupArena`]
//! - [`Map`]: generic set of values based on [`BTreeMap`]
//! - [`Set`]: generic key-value mapping based on [`BTreeSet`]
//! - [`StringInterner`]: stores and deduplicates strings efficiently
//!
//! [`BTreeSet`]: std::collections::BTreeSet
//! [`BTreeMap`]: std::collections::BTreeMap

#![warn(
    clippy::cast_lossless,
    clippy::missing_errors_doc,
    clippy::used_underscore_binding,
    clippy::redundant_closure_for_method_calls,
    clippy::type_repetition_in_bounds,
    clippy::inconsistent_struct_constructor,
    clippy::default_trait_access,
    clippy::map_unwrap_or,
    clippy::items_after_statements
)]

pub mod arena;
mod head_vec;
pub mod map;
pub mod set;
pub mod string_interner;

#[cfg(test)]
mod tests;

#[doc(inline)]
#[allow(unused_imports)]
pub use self::{
    arena::{Arena, ComponentVec, DedupArena},
    head_vec::HeadVec,
    map::Map,
    set::Set,
    string_interner::StringInterner,
};
