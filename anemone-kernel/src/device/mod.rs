//! Device module, which contains code for initializing and managing devices.

pub mod discovery;

mod serial;

pub trait PlatformDevice {}
