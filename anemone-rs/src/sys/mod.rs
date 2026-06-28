//! Internal system backends, providing most primitive syscalls.
//!
//! We won't support multiple OSes in the near future. This standard library
//! only targets Anemone, and leverages many Anemone-specific features. So it's
//! not intended to be portable. And note that in our upper-level os-agnostic
//! APIs, we often mix Anemone and linux features together.
//!
//! Anemone itself is linux-compatible, so we split those linux-compatible codes
//! into `linux` module, while the `anemone` module is for Anemone-specific
//! features.
//!
//! In fact, using those linux-compatible APIs would be pretty enough to develop
//! this library. But once again - portability is not our concern. If we want
//! that, why we don't just use `std`?
//!
//! TODO: a huge bug may exist. keywords: aliasing rules, llvm optimization,
//! mutable reference and pointer. Fix this later.

#![allow(unused)]

// our native kernel. (●'◡'●)
pub mod anemone;
pub mod linux;
