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

#[derive(Deserialize, Debug, Serialize, Clone, Copy, PartialEq, Eq)]
pub enum SchedDefaultPolicy {
    #[serde(rename = "fair")]
    Fair,
    #[serde(rename = "rt_rr")]
    RtRr,
    #[serde(rename = "rt_fifo")]
    RtFifo,
}

impl SchedDefaultPolicy {
    fn kernel_variant(self) -> &'static str {
        match self {
            Self::Fair => "Fair",
            Self::RtRr => "RtRr",
            Self::RtFifo => "RtFifo",
        }
    }
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Build {
    pub platform: String,
    pub profile: Profile,

    pub disasm: bool,
}

#[derive(Deserialize, Debug, Serialize, PartialEq, Eq)]
pub struct Parameters {
    pub bootstrap_heap_shift_kb: Option<u64>,
    pub log_buffer_shift_kb: Option<u64>,
    pub log_record_shift_bytes: Option<u64>,
    pub print_log_level: Option<u8>,
    pub record_log_level: Option<u8>,
    pub kstack_shift_kb: Option<u64>,
    pub remap_shift_gb: Option<u64>,
    pub max_logical_cpus: Option<usize>,
    pub max_ident_len_bytes: Option<usize>,
    pub max_path_len_bytes: Option<usize>,
    pub max_processes: Option<u64>,
    pub system_hz: Option<u16>,
    pub sched_default_policy: Option<SchedDefaultPolicy>,
    pub rt_rr_timeslice_ms: Option<u64>,
    pub backtrace_depth: Option<usize>,
    pub user_stack_shift_kb: Option<u64>,
    pub user_init_stack_shift_kb: Option<u64>,
    pub user_heap_shift_mb: Option<u64>,
    pub shmmax_bytes: Option<usize>,
    pub shmall_pages: Option<usize>,
    pub shmmni: Option<usize>,
    pub io_shrink_threshold: Option<u8>,
    pub oom_kill_threshold: Option<u8>,
    pub symlink_resolve_limit: Option<usize>,
    pub max_fd_per_process: Option<usize>,
    pub ramdisk_count: Option<usize>,
    pub loop_device_count: Option<usize>,
    pub ns16550a_default_baud: Option<u32>,
    pub dw_mshc_poll_timeout_ms: Option<u64>,
    pub eevdf_base_slice_us: Option<u64>,
    pub eevdf_wake_clamp_us: Option<u64>,
    pub eevdf_yield_penalty_us: Option<u64>,
    pub eevdf_anomaly_threshold: Option<u64>,
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

        let record_log_level = default_or!(record_log_level);
        // Printing is downstream of recording, so it cannot admit a level that
        // the record gate has already turned into a no-op.
        let print_log_level = default_or!(print_log_level).min(record_log_level);

        format!(
            r#"//! Auto-generated kernel parameters from kconfig, do not edit manually.
#![allow(unused)]

/// Size of bootstrap heap as a power of 2 in KB
pub const BOOTSTRAP_HEAP_SHIFT_KB: u64 = {};
/// Log buffer size as a power of 2 in KB, excluding metadata overhead
pub const LOG_BUFFER_SHIFT_KB: u64 = {};
/// Log record size as a power of 2 in bytes
/// Note that the actual log record size will be 
/// 2^LOG_RECORD_SHIFT_BYTES + some metadata overhead.
pub const LOG_RECORD_SHIFT_BYTES: u64 = {};
/// Maximum numeric log level that may be printed to consoles.
///
/// Log levels follow the kernel ordering: Emerg=0 ... Debug=7.
/// This value is capped by `RECORD_LOG_LEVEL` because an unrecorded
/// message is a no-op and cannot be printed.
pub const PRINT_LOG_LEVEL: u8 = {};
/// Maximum numeric log level that may enter the kernel log buffer.
///
/// Messages with a numerically larger level are complete no-ops.
pub const RECORD_LOG_LEVEL: u8 = {};
/// Kernel stack size as a power of 2 in KB
pub const KSTACK_SHIFT_KB: u64 = {};
/// Remap region size as a power of 2 in GB
pub const REMAP_SHIFT_GB: u64 = {};
/// Maximum number of logical CPUs enabled by this kernel
pub const MAX_LOGICAL_CPUS: usize = {};
/// Maximum length of identity strings in bytes
pub const MAX_IDENT_LEN_BYTES: usize = {};
/// Maximum length of file names in bytes. This is always equal to 
/// MAX_IDENT_LEN_BYTES,
/// since file names are commonly used as identity strings in kernel 
/// objects.
pub const MAX_FILE_NAME_LEN_BYTES: usize = MAX_IDENT_LEN_BYTES;
/// Maximum length of file paths in bytes
pub const MAX_PATH_LEN_BYTES: usize = {};
/// Maximum number of processes
pub const MAX_PROCESSES: u64 = {};
/// System timer frequency in hertz, i.e. number of timer interrupts 
/// per second
pub const SYSTEM_HZ: u16 = {};
/// Compile-time scheduler policy for fresh non-idle tasks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedDefaultPolicy {{
    Fair,
    RtRr,
    RtFifo,
}}
/// Selected compile-time scheduler policy for fresh non-idle tasks.
pub const SCHED_DEFAULT_POLICY: SchedDefaultPolicy = SchedDefaultPolicy::{};
/// RT/RR timeslice target in milliseconds.
pub const RT_RR_TIMESLICE_MS: u64 = {};
/// Maximum depth of captured backtrace
pub const BACKTRACE_DEPTH: usize = {};
/// Max user stack size as a power of 2 in KB
pub const USER_STACK_SHIFT_KB: u64 = {};
/// Initial user stack size as a power of 2 in KB
pub const USER_INIT_STACK_SHIFT_KB: u64 = {};
/// Max user heap size as a power of 2 in MB
pub const USER_HEAP_SHIFT_MB: u64 = {};
/// Default maximum size in bytes for a single System V shared memory 
/// segment.
pub const SHMMAX: usize = {};
/// Default maximum number of pages that may be allocated to System V 
/// shared memory.
pub const SHMALL: usize = {};
/// Default maximum number of System V shared memory segments.
pub const SHMMNI: usize = {};
/// Physical memory usage percentage above which the inode shrinker worker 
/// runs a scan.
pub const IO_SHRINK_THRESHOLD: u8 = {};
/// Physical memory usage percentage above which the OOM killer worker 
/// is woken.
pub const OOM_KILL_THRESHOLD: u8 = {};
/// Maximum number of symbolic links to resolve in a single path resolution
pub const SYMLINK_RESOLVE_LIMIT: usize = {};
/// Default maximum number of file descriptors per process. 
/// Might be overridden by certain syscalls.
pub const MAX_FD_PER_PROCESS: usize = {};
/// Number of static ramdisk block devices to publish at boot.
pub const RAMDISK_COUNT: usize = {};
/// Number of static loop block devices to publish at boot.
pub const LOOP_DEVICE_COUNT: usize = {};
/// Default NS16550A baud used when stdout-path has no device-specific options.
pub const NS16550A_DEFAULT_BAUD: u32 = {};
/// Bounded DW-MSHC register polling timeout in milliseconds.
pub const DW_MSHC_POLL_TIMEOUT_MS: u64 = {};
/// EEVDF-lite base slice in microseconds.
pub const EEVDF_BASE_SLICE_US: u64 = {};
/// EEVDF-lite wake placement clamp window in microseconds.
pub const EEVDF_WAKE_CLAMP_US: u64 = {};
/// EEVDF-lite bounded yield penalty window in microseconds.
pub const EEVDF_YIELD_PENALTY_US: u64 = {};
/// Consecutive EEVDF no-eligible fallback count before an extra error summary.
pub const EEVDF_ANOMALY_THRESHOLD: u64 = {};
        "#,
            default_or!(bootstrap_heap_shift_kb),
            default_or!(log_buffer_shift_kb),
            default_or!(log_record_shift_bytes),
            print_log_level,
            record_log_level,
            default_or!(kstack_shift_kb),
            default_or!(remap_shift_gb),
            default_or!(max_logical_cpus),
            default_or!(max_ident_len_bytes),
            default_or!(max_path_len_bytes),
            default_or!(max_processes),
            default_or!(system_hz),
            default_or!(sched_default_policy).kernel_variant(),
            default_or!(rt_rr_timeslice_ms),
            default_or!(backtrace_depth),
            default_or!(user_stack_shift_kb),
            default_or!(user_init_stack_shift_kb),
            default_or!(user_heap_shift_mb),
            default_or!(shmmax_bytes),
            default_or!(shmall_pages),
            default_or!(shmmni),
            default_or!(io_shrink_threshold),
            default_or!(oom_kill_threshold),
            default_or!(symlink_resolve_limit),
            default_or!(max_fd_per_process),
            default_or!(ramdisk_count),
            default_or!(loop_device_count),
            default_or!(ns16550a_default_baud),
            default_or!(dw_mshc_poll_timeout_ms),
            default_or!(eevdf_base_slice_us),
            default_or!(eevdf_wake_clamp_us),
            default_or!(eevdf_yield_penalty_us),
            default_or!(eevdf_anomaly_threshold),
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
        if config.parameters.rt_rr_timeslice_ms == Some(0) {
            anyhow::bail!("rt_rr_timeslice_ms must be non-zero");
        }
        if config.parameters.dw_mshc_poll_timeout_ms == Some(0) {
            anyhow::bail!("dw_mshc_poll_timeout_ms must be non-zero");
        }
        if config
            .parameters
            .print_log_level
            .is_some_and(|level| level > 7)
        {
            anyhow::bail!("print_log_level must be in the range 0..=7");
        }
        if config
            .parameters
            .record_log_level
            .is_some_and(|level| level > 7)
        {
            anyhow::bail!("record_log_level must be in the range 0..=7");
        }
        Ok(config)
    }

    pub fn into_kernel_config(self) -> KernelConfig {
        KernelConfig {
            features: self.features,
            parameters: self.parameters,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct KernelConfig {
    pub features: HashMap<String, bool>,
    pub parameters: Parameters,
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

    #[test]
    fn test_sched_parameters_are_constrained() {
        let content = std::fs::read_to_string("../../conf/.defconfig").unwrap();
        let replace_parameter = |name: &str, replacement: &str| {
            content
                .lines()
                .map(|line| {
                    if line.trim_start().starts_with(name) {
                        replacement
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        assert!(
            Config::from_str(
                &replace_parameter("sched_default_policy", "sched_default_policy = \"fair\"")
            )
            .is_ok()
        );
        assert!(
            Config::from_str(
                &replace_parameter("sched_default_policy", "sched_default_policy = \"rt_rr\"")
            )
            .is_ok()
        );
        assert!(
            Config::from_str(
                &replace_parameter("sched_default_policy", "sched_default_policy = \"rt_fifo\"")
            )
            .is_ok()
        );
        assert!(
            Config::from_str(
                &replace_parameter("sched_default_policy", "sched_default_policy = \"invalid\"")
            )
            .is_err()
        );
        assert!(
            Config::from_str(&replace_parameter(
                "rt_rr_timeslice_ms",
                "rt_rr_timeslice_ms = 0"
            ))
            .is_err()
        );
    }

    #[test]
    fn test_dw_mshc_parameter_is_constrained() {
        let content = std::fs::read_to_string("../../conf/.defconfig").unwrap();
        let replace_parameter = |name: &str, replacement: &str| {
            content
                .lines()
                .map(|line| {
                    if line.trim_start().starts_with(name) {
                        replacement
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        assert!(
            Config::from_str(&replace_parameter(
                "dw_mshc_poll_timeout_ms",
                "dw_mshc_poll_timeout_ms = 0"
            ))
            .is_err()
        );
    }

    #[test]
    fn test_log_levels_are_constrained() {
        let content = std::fs::read_to_string("../../conf/.defconfig").unwrap();
        let replace_parameter = |name: &str, replacement: &str| {
            content
                .lines()
                .map(|line| {
                    if line.trim_start().starts_with(name) {
                        replacement
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        assert!(
            Config::from_str(&replace_parameter("print_log_level", "print_log_level = 8"))
                .is_err()
        );
        assert!(
            Config::from_str(&replace_parameter("record_log_level", "record_log_level = 8"))
                .is_err()
        );
    }
}
