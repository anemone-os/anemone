/// Toolchains and other build-related configuration.
use crate::config::platform::*;

impl TargetTriple {
    pub fn objdump(&self) -> &'static str {
        match self {
            Self::RiscV64UnknownAnemoneElf => {
                if cfg!(target_os = "linux") {
                    "riscv64-unknown-elf-objdump"
                } else if cfg!(target_os = "macos") {
                    "riscv64-elf-objdump"
                } else {
                    unimplemented!("Objdump for target triple {:?} is not implemented on this platform", self)
                }
            },
            _ => unimplemented!("Objdump for target triple {:?} is not implemented", self),
        }
    }

    pub fn objcopy(&self) -> &'static str {
        match self {
            Self::RiscV64UnknownAnemoneElf => {
                if cfg!(target_os = "linux") {
                    "riscv64-unknown-elf-objcopy"
                } else if cfg!(target_os = "macos") {
                    "riscv64-elf-objcopy"
                } else {
                    unimplemented!("Objcopy for target triple {:?} is not implemented on this platform", self)
                }
            },
            _ => unimplemented!("Objcopy for target triple {:?} is not implemented", self),
        }
    }
}
