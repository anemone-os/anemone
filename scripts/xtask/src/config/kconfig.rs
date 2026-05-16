//! This module is responsible for handling the top level
//! kernel configuration file `conf/kconfig.toml`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::workspace::*;

#[derive(Deserialize, Debug, Serialize)]
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

#[derive(Deserialize, Debug, Serialize)]
pub struct Build {
    pub platform: String,
    pub profile: Profile,

    pub disasm: bool,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Parameters {
    pub bootstrap_heap_shift_kb: Option<u64>,
    pub log_buffer_shift_kb: Option<u64>,
    pub log_record_shift_bytes: Option<u64>,
    pub console_log_level: Option<u8>,
    pub kstack_shift_kb: Option<u64>,
    pub remap_shift_gb: Option<u64>,
    pub max_ident_len_bytes: Option<usize>,
    pub max_path_len_bytes: Option<usize>,
    pub max_processes: Option<u64>,
    pub system_hz: Option<u16>,
    pub backtrace_depth: Option<usize>,
    pub user_stack_shift_kb: Option<u64>,
    pub user_init_stack_shift_kb: Option<u64>,
    pub user_heap_shift_mb: Option<u64>,
    pub symlink_resolve_limit: Option<usize>,
    pub max_fd_per_process: Option<usize>,
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
/// Maximum numeric log level that may be emitted to consoles.
///
/// Log levels follow the kernel ordering: Emerg=0 ... Debug=7.
/// Messages with a numerically larger level stay in the kernel log buffer only.
pub const CONSOLE_LOG_LEVEL: u8 = {};
/// Kernel stack size as a power of 2 in KB
pub const KSTACK_SHIFT_KB: u64 = {};
/// Remap region size as a power of 2 in GB
pub const REMAP_SHIFT_GB: u64 = {};
/// Maximum length of identity strings in bytes
pub const MAX_IDENT_LEN_BYTES: usize = {};
/// Maximum length of file names in bytes. This is always equal to MAX_IDENT_LEN_BYTES,
/// since file names are commonly used as identity strings in kernel objects.
pub const MAX_FILE_NAME_LEN_BYTES: usize = MAX_IDENT_LEN_BYTES;
/// Maximum length of file paths in bytes
pub const MAX_PATH_LEN_BYTES: usize = {};
/// Maximum number of processes
pub const MAX_PROCESSES: u64 = {};
/// System timer frequency in hertz, i.e. number of timer interrupts per second
pub const SYSTEM_HZ: u16 = {};
/// Maximum depth of captured backtrace
pub const BACKTRACE_DEPTH: usize = {};
/// Max user stack size as a power of 2 in KB
pub const USER_STACK_SHIFT_KB: u64 = {};
/// Initial user stack size as a power of 2 in KB
pub const USER_INIT_STACK_SHIFT_KB: u64 = {};
/// Max user heap size as a power of 2 in MB
pub const USER_HEAP_SHIFT_MB: u64 = {};
/// Maximum number of symbolic links to resolve in a single path resolution
pub const SYMLINK_RESOLVE_LIMIT: usize = {};
/// Default maximum number of file descriptors per process. Might be overridden by certain syscalls.
pub const MAX_FD_PER_PROCESS: usize = {};
        "#,
            default_or!(bootstrap_heap_shift_kb),
            default_or!(log_buffer_shift_kb),
            default_or!(log_record_shift_bytes),
            default_or!(console_log_level),
            default_or!(kstack_shift_kb),
            default_or!(remap_shift_gb),
            default_or!(max_ident_len_bytes),
            default_or!(max_path_len_bytes),
            default_or!(max_processes),
            default_or!(system_hz),
            default_or!(backtrace_depth),
            default_or!(user_stack_shift_kb),
            default_or!(user_init_stack_shift_kb),
            default_or!(user_heap_shift_mb),
            default_or!(symlink_resolve_limit),
            default_or!(max_fd_per_process),
        )
    }
}

#[derive(Deserialize, Debug, Serialize)]
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
