//! This module is responsible for handling the top level
//! kernel configuration file `conf/kconfig.toml`.

use std::collections::HashMap;

use serde::Deserialize;

use crate::workspace::*;

#[derive(Deserialize, Debug)]
pub enum Profile {
    // debug with some minor customizations
    #[serde(rename = "dev")]
    Dev,
    #[serde(rename = "release")]
    Release,
}

impl Profile {
    pub fn as_cargo_arg(&self) -> &'static [&'static str] {
        match self {
            Profile::Dev => &["--profile", "dev"],
            Profile::Release => &["--release"],
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct Build {
    pub platform: String,
    pub profile: Profile,
}

#[derive(Deserialize, Debug)]
pub struct Parameters {
    pub bootstrap_heap_shift_kb: Option<u64>,
    pub log_buffer_shift_kb: Option<u64>,
    pub log_record_shift_bytes: Option<u64>,
    pub kstack_shift_kb: Option<u64>,
    pub remap_shift_gb: Option<u64>,
    pub max_ident_len_bytes: Option<usize>,
    pub max_processes: Option<u64>,
    pub time_slice_ms: Option<u64>,
    pub system_hz: Option<u16>,
}

impl Parameters {
    /// Generate Rust definitions for kernel parameters
    /// to be included in the kernel build.
    ///
    /// P.S. Can we do some metaprogramming here to avoid manual updates?
    pub fn gen_kconfig_defs(&self) -> String {
        let defconfig_content =
            std::fs::read_to_string(DEF_KCONFIG_PATH).expect("Failed to read default kconfig");
        let defconfig =
            Config::from_str(&defconfig_content).expect("Failed to parse default kconfig");

        macro_rules! default_or {
            ($field:ident) => {
                self.$field
                    .unwrap_or(defconfig.parameters.$field.expect(&format!(
                        "Default value for {} must be specified in {}",
                        stringify!($field),
                        DEF_KCONFIG_PATH
                    )))
            };
        }

        format!(
            r#"//! Auto-generated kernel parameters from kconfig, do not edit manually.
#![allow(unused)]

/// Size of bootstrap heap as a power of 2 in KB
pub const BOOTSTRAP_HEAP_SHIFT_KB: u64 = {};
/// Log buffer size as a power of 2 in KB, excluding metadata overhead
pub const LOG_BUFFER_SHIFT_KB: u64 = {};
/// Log record size as a power of 2 in bytes
/// Note that the actual log record size will be 2^LOG_RECORD_SHIFT_BYTES + some metadata overhead.
pub const LOG_RECORD_SHIFT_BYTES: u64 = {};
/// Kernel stack size as a power of 2 in KB
pub const KSTACK_SHIFT_KB: u64 = {};
/// Remap region size as a power of 2 in GB
pub const REMAP_SHIFT_GB: u64 = {};
/// Maximum length of identity strings in bytes
pub const MAX_IDENT_LEN_BYTES: usize = {};
/// Maximum length of file names in bytes. This is always equal to MAX_IDENT_LEN_BYTES,
/// since file names are commonly used as identity strings in kernel objects.
pub const MAX_FILE_NAME_LEN_BYTES: usize = MAX_IDENT_LEN_BYTES;
/// Maximum number of processes
pub const MAX_PROCESSES: u64 = {};
/// Time slice duration in milliseconds
pub const TIME_SLICE_MS: u64 = {};
/// System timer frequency in hertz, i.e. number of timer interrupts per second
pub const SYSTEM_HZ: u16 = {};
        "#,
            default_or!(bootstrap_heap_shift_kb),
            default_or!(log_buffer_shift_kb),
            default_or!(log_record_shift_bytes),
            default_or!(kstack_shift_kb),
            default_or!(remap_shift_gb),
            default_or!(max_ident_len_bytes),
            default_or!(max_processes),
            default_or!(time_slice_ms),
            default_or!(system_hz)
        )
    }
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub build: Build,
    pub features: HashMap<String, bool>,
    pub parameters: Parameters,
}

impl Config {
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsing() {
        let content = std::fs::read_to_string("../../conf/.defconfig").unwrap();
        let config = Config::from_str(&content).unwrap();
        println!("{:#x?}", config);
    }
}
