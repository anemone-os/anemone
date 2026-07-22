//! This module provides utilities for resolving and creating
//! configuration files for the Anemone build system.

pub mod app;
pub mod kconfig;
pub mod platform;
pub mod reference;
pub mod resolve;
pub mod rootfs;
pub mod system_target;

pub mod build;

pub use kconfig::Config as KConfig;
pub use platform::Config as PlatformConfig;
