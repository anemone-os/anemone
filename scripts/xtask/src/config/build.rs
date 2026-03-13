/// Toolchains and other build-related configuration.
use crate::config::platform::*;

impl TargetTriple {
    pub fn objdump(&self) -> &'static str {
        match self {
            Self::RiscV64UnknownAnemoneElf => "llvm-objdump",
            _ => unimplemented!("Objdump for target triple {:?} is not implemented", self),
        }
    }

    pub fn objcopy(&self) -> &'static str {
        match self {
            Self::RiscV64UnknownAnemoneElf => "llvm-objcopy",
            _ => unimplemented!("Objcopy for target triple {:?} is not implemented", self),
        }
    }
}
