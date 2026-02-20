//! This module provides utilities for resolving and creating
//! configuration files for the Anemone build system.

pub mod kconfig;
pub mod platform;

pub mod build;

pub use kconfig::Config as KConfig;
pub use platform::Config as PlatformConfig;
