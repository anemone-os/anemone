/// Toolchains and other build-related configuration.
use crate::config::platform::*;

impl TargetTriple {
    pub fn objdump(&self) -> &'static str {
        "rust-objdump"
    }

    pub fn objcopy(&self) -> &'static str {
        "rust-objcopy"
    }
}
