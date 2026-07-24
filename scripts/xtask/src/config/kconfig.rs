//! This module is responsible for handling the top level
//! kernel configuration file `conf/kconfig.toml`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::workspace::*;

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
    pub tty_raw_rx_capacity_bytes: Option<usize>,
    pub tty_canonical_line_capacity_bytes: Option<usize>,
    pub tty_input_capacity_bytes: Option<usize>,
    pub tty_output_capacity_bytes: Option<usize>,
    pub tty_worker_batch_bytes: Option<usize>,
    pub ns16550a_irq_rx_budget_bytes: Option<usize>,
    pub ns16550a_tx_batch_bytes: Option<usize>,
    pub ns16550a_tx_poll_iterations: Option<usize>,
    pub dw_mshc_poll_timeout_ms: Option<u64>,
    pub ahci_hba_reset_timeout_ms: Option<u64>,
    pub ahci_engine_timeout_ms: Option<u64>,
    pub ahci_port_timeout_ms: Option<u64>,
    pub ahci_command_timeout_ms: Option<u64>,
    pub ahci_read_warn_ms: Option<u64>,
    pub ahci_read_timeout_ms: Option<u64>,
    pub ahci_bounce_kb: Option<usize>,
    pub eevdf_base_slice_us: Option<u64>,
    pub eevdf_wake_clamp_us: Option<u64>,
    pub eevdf_yield_penalty_us: Option<u64>,
    pub eevdf_anomaly_threshold: Option<u64>,
}

impl Parameters {
    /// Materialize the optional parameter syntax into the complete value owned
    /// by a resolved KernelConfig. Build consumers must not consult
    /// `.defconfig` after this boundary.
    pub(super) fn materialize_defaults(&mut self, defaults: Option<&Self>) -> anyhow::Result<()> {
        macro_rules! materialize {
            ($field:ident) => {
                if self.$field.is_none() {
                    self.$field =
                        Some(defaults.and_then(|value| value.$field).ok_or_else(|| {
                            anyhow::anyhow!(
                                "default value for {} must be specified in {}",
                                stringify!($field),
                                DEF_KCONFIG_PATH
                            )
                        })?);
                }
            };
        }

        materialize!(bootstrap_heap_shift_kb);
        materialize!(log_buffer_shift_kb);
        materialize!(log_record_shift_bytes);
        materialize!(print_log_level);
        materialize!(record_log_level);
        materialize!(kstack_shift_kb);
        materialize!(remap_shift_gb);
        materialize!(max_logical_cpus);
        materialize!(max_ident_len_bytes);
        materialize!(max_path_len_bytes);
        materialize!(max_processes);
        materialize!(system_hz);
        materialize!(sched_default_policy);
        materialize!(rt_rr_timeslice_ms);
        materialize!(backtrace_depth);
        materialize!(user_stack_shift_kb);
        materialize!(user_init_stack_shift_kb);
        materialize!(user_heap_shift_mb);
        materialize!(shmmax_bytes);
        materialize!(shmall_pages);
        materialize!(shmmni);
        materialize!(io_shrink_threshold);
        materialize!(oom_kill_threshold);
        materialize!(symlink_resolve_limit);
        materialize!(max_fd_per_process);
        materialize!(ramdisk_count);
        materialize!(loop_device_count);
        materialize!(ns16550a_default_baud);
        materialize!(tty_raw_rx_capacity_bytes);
        materialize!(tty_canonical_line_capacity_bytes);
        materialize!(tty_input_capacity_bytes);
        materialize!(tty_output_capacity_bytes);
        materialize!(tty_worker_batch_bytes);
        materialize!(ns16550a_irq_rx_budget_bytes);
        materialize!(ns16550a_tx_batch_bytes);
        materialize!(ns16550a_tx_poll_iterations);
        materialize!(dw_mshc_poll_timeout_ms);
        materialize!(ahci_hba_reset_timeout_ms);
        materialize!(ahci_engine_timeout_ms);
        materialize!(ahci_port_timeout_ms);
        materialize!(ahci_command_timeout_ms);
        materialize!(ahci_read_warn_ms);
        materialize!(ahci_read_timeout_ms);
        materialize!(ahci_bounce_kb);
        materialize!(eevdf_base_slice_us);
        materialize!(eevdf_wake_clamp_us);
        materialize!(eevdf_yield_penalty_us);
        materialize!(eevdf_anomaly_threshold);
        Ok(())
    }

    /// Generate Rust definitions for kernel parameters
    /// to be included in the kernel build.
    ///
    /// P.S. Can we do some metaprogramming here to avoid manual updates?
    pub fn gen_kconfig_defs(&self) -> String {
        macro_rules! resolved {
            ($field:ident) => {
                self.$field.unwrap_or_else(|| {
                    panic!(
                        "resolved KernelConfig is missing parameter {}",
                        stringify!($field),
                    )
                })
            };
        }

        let record_log_level = resolved!(record_log_level);
        // Printing is downstream of recording, so it cannot admit a level that
        // the record gate has already turned into a no-op.
        let print_log_level = resolved!(print_log_level).min(record_log_level);

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
/// Per-port fixed raw TTY RX FIFO capacity in bytes.
pub const TTY_RAW_RX_CAPACITY_BYTES: usize = {};
/// Maximum canonical TTY line size including its delimiter.
pub const TTY_CANONICAL_LINE_CAPACITY_BYTES: usize = {};
/// Per-Terminal committed input capacity in bytes.
pub const TTY_INPUT_CAPACITY_BYTES: usize = {};
/// Per-Terminal transformed output capacity in bytes.
pub const TTY_OUTPUT_CAPACITY_BYTES: usize = {};
/// Maximum RX/TX bytes advanced by one endpoint worker batch.
pub const TTY_WORKER_BATCH_BYTES: usize = {};
/// Maximum RX bytes drained by one NS16550A IRQ handler invocation.
pub const NS16550A_IRQ_RX_BUDGET_BYTES: usize = {};
/// Maximum bytes submitted while holding the NS16550A TX lock.
pub const NS16550A_TX_BATCH_BYTES: usize = {};
/// Maximum readiness polls for each NS16550A TX byte.
pub const NS16550A_TX_POLL_ITERATIONS: usize = {};
/// Bounded DW-MSHC register polling timeout in milliseconds.
pub const DW_MSHC_POLL_TIMEOUT_MS: u64 = {};
/// AHCI global reset deadline in milliseconds.
pub const AHCI_HBA_RESET_TIMEOUT_MS: u64 = {};
/// AHCI command-list/FIS engine transition deadline in milliseconds.
pub const AHCI_ENGINE_TIMEOUT_MS: u64 = {};
/// AHCI link and device-ready deadline in milliseconds.
pub const AHCI_PORT_TIMEOUT_MS: u64 = {};
/// AHCI ATA command completion deadline in milliseconds.
pub const AHCI_COMMAND_TIMEOUT_MS: u64 = {};
/// ATA read latency threshold for emitting a warning.
pub const AHCI_READ_WARN_MS: u64 = {};
/// ATA read deadline after which the kernel panics.
pub const AHCI_READ_TIMEOUT_MS: u64 = {};
/// Per-port AHCI DMA bounce buffer size in KiB.
pub const AHCI_BOUNCE_KB: usize = {};
/// EEVDF-lite base slice in microseconds.
pub const EEVDF_BASE_SLICE_US: u64 = {};
/// EEVDF-lite wake placement clamp window in microseconds.
pub const EEVDF_WAKE_CLAMP_US: u64 = {};
/// EEVDF-lite bounded yield penalty window in microseconds.
pub const EEVDF_YIELD_PENALTY_US: u64 = {};
/// Consecutive EEVDF no-eligible fallback count before an extra error summary.
pub const EEVDF_ANOMALY_THRESHOLD: u64 = {};
"#,
            resolved!(bootstrap_heap_shift_kb),
            resolved!(log_buffer_shift_kb),
            resolved!(log_record_shift_bytes),
            print_log_level,
            record_log_level,
            resolved!(kstack_shift_kb),
            resolved!(remap_shift_gb),
            resolved!(max_logical_cpus),
            resolved!(max_ident_len_bytes),
            resolved!(max_path_len_bytes),
            resolved!(max_processes),
            resolved!(system_hz),
            resolved!(sched_default_policy).kernel_variant(),
            resolved!(rt_rr_timeslice_ms),
            resolved!(backtrace_depth),
            resolved!(user_stack_shift_kb),
            resolved!(user_init_stack_shift_kb),
            resolved!(user_heap_shift_mb),
            resolved!(shmmax_bytes),
            resolved!(shmall_pages),
            resolved!(shmmni),
            resolved!(io_shrink_threshold),
            resolved!(oom_kill_threshold),
            resolved!(symlink_resolve_limit),
            resolved!(max_fd_per_process),
            resolved!(ramdisk_count),
            resolved!(loop_device_count),
            resolved!(ns16550a_default_baud),
            resolved!(tty_raw_rx_capacity_bytes),
            resolved!(tty_canonical_line_capacity_bytes),
            resolved!(tty_input_capacity_bytes),
            resolved!(tty_output_capacity_bytes),
            resolved!(tty_worker_batch_bytes),
            resolved!(ns16550a_irq_rx_budget_bytes),
            resolved!(ns16550a_tx_batch_bytes),
            resolved!(ns16550a_tx_poll_iterations),
            resolved!(dw_mshc_poll_timeout_ms),
            resolved!(ahci_hba_reset_timeout_ms),
            resolved!(ahci_engine_timeout_ms),
            resolved!(ahci_port_timeout_ms),
            resolved!(ahci_command_timeout_ms),
            resolved!(ahci_read_warn_ms),
            resolved!(ahci_read_timeout_ms),
            resolved!(ahci_bounce_kb),
            resolved!(eevdf_base_slice_us),
            resolved!(eevdf_wake_clamp_us),
            resolved!(eevdf_yield_penalty_us),
            resolved!(eevdf_anomaly_threshold),
        )
    }
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
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
        for (name, value) in [
            (
                "ahci_hba_reset_timeout_ms",
                config.parameters.ahci_hba_reset_timeout_ms,
            ),
            (
                "ahci_engine_timeout_ms",
                config.parameters.ahci_engine_timeout_ms,
            ),
            (
                "ahci_port_timeout_ms",
                config.parameters.ahci_port_timeout_ms,
            ),
            (
                "ahci_command_timeout_ms",
                config.parameters.ahci_command_timeout_ms,
            ),
            ("ahci_read_warn_ms", config.parameters.ahci_read_warn_ms),
            (
                "ahci_read_timeout_ms",
                config.parameters.ahci_read_timeout_ms,
            ),
        ] {
            if value == Some(0) {
                anyhow::bail!("{name} must be non-zero");
            }
        }
        if matches!(
            (
                config.parameters.ahci_read_warn_ms,
                config.parameters.ahci_read_timeout_ms,
            ),
            (Some(warn), Some(timeout)) if warn >= timeout
        ) {
            anyhow::bail!("ahci_read_warn_ms must be less than ahci_read_timeout_ms");
        }
        if config
            .parameters
            .ahci_bounce_kb
            .is_some_and(|size| size == 0 || size > 4096)
        {
            anyhow::bail!("ahci_bounce_kb must be in the range 1..=4096");
        }
        for (name, value) in [
            (
                "tty_raw_rx_capacity_bytes",
                config.parameters.tty_raw_rx_capacity_bytes,
            ),
            (
                "ns16550a_irq_rx_budget_bytes",
                config.parameters.ns16550a_irq_rx_budget_bytes,
            ),
            (
                "ns16550a_tx_batch_bytes",
                config.parameters.ns16550a_tx_batch_bytes,
            ),
            (
                "ns16550a_tx_poll_iterations",
                config.parameters.ns16550a_tx_poll_iterations,
            ),
        ] {
            if value == Some(0) {
                anyhow::bail!("{name} must be non-zero");
            }
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
            Config::from_str(&replace_parameter(
                "sched_default_policy",
                "sched_default_policy = \"fair\""
            ))
            .is_ok()
        );
        assert!(
            Config::from_str(&replace_parameter(
                "sched_default_policy",
                "sched_default_policy = \"rt_rr\""
            ))
            .is_ok()
        );
        assert!(
            Config::from_str(&replace_parameter(
                "sched_default_policy",
                "sched_default_policy = \"rt_fifo\""
            ))
            .is_ok()
        );
        assert!(
            Config::from_str(&replace_parameter(
                "sched_default_policy",
                "sched_default_policy = \"invalid\""
            ))
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
    fn test_tty_transport_parameters_are_constrained_and_defaulted() {
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

        for name in [
            "tty_raw_rx_capacity_bytes",
            "ns16550a_irq_rx_budget_bytes",
            "ns16550a_tx_batch_bytes",
            "ns16550a_tx_poll_iterations",
        ] {
            assert!(
                Config::from_str(&replace_parameter(name, &format!("{name} = 0"))).is_err(),
                "{name} accepted zero"
            );
        }
        let mut config = Config::from_str(&content).unwrap();
        config.parameters.tty_raw_rx_capacity_bytes = None;
        config.parameters.ns16550a_irq_rx_budget_bytes = None;
        config.parameters.ns16550a_tx_batch_bytes = None;
        config.parameters.ns16550a_tx_poll_iterations = None;
        let defaults = Config::from_str(&content).unwrap();
        config
            .parameters
            .materialize_defaults(Some(&defaults.parameters))
            .unwrap();
        let defs = config.parameters.gen_kconfig_defs();
        assert!(defs.contains("pub const TTY_RAW_RX_CAPACITY_BYTES: usize = 4096;"));
        assert!(defs.contains("pub const NS16550A_IRQ_RX_BUDGET_BYTES: usize = 256;"));
        assert!(defs.contains("pub const NS16550A_TX_BATCH_BYTES: usize = 16;"));
        assert!(defs.contains("pub const NS16550A_TX_POLL_ITERATIONS: usize = 65536;"));
    }

    #[test]
    fn test_tty_terminal_parameters_are_defaulted() {
        let content = std::fs::read_to_string("../../conf/.defconfig").unwrap();
        let mut config = Config::from_str(&content).unwrap();
        config.parameters.tty_canonical_line_capacity_bytes = None;
        config.parameters.tty_input_capacity_bytes = None;
        config.parameters.tty_output_capacity_bytes = None;
        config.parameters.tty_worker_batch_bytes = None;
        let defaults = Config::from_str(&content).unwrap();
        config
            .parameters
            .materialize_defaults(Some(&defaults.parameters))
            .unwrap();
        let defs = config.parameters.gen_kconfig_defs();
        assert!(defs.contains("pub const TTY_CANONICAL_LINE_CAPACITY_BYTES: usize = 4096;"));
        assert!(defs.contains("pub const TTY_INPUT_CAPACITY_BYTES: usize = 4096;"));
        assert!(defs.contains("pub const TTY_OUTPUT_CAPACITY_BYTES: usize = 4096;"));
        assert!(defs.contains("pub const TTY_WORKER_BATCH_BYTES: usize = 256;"));
    }

    #[test]
    fn test_ahci_parameters_are_constrained_and_defaulted() {
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

        for name in [
            "ahci_hba_reset_timeout_ms",
            "ahci_engine_timeout_ms",
            "ahci_port_timeout_ms",
            "ahci_command_timeout_ms",
            "ahci_read_warn_ms",
            "ahci_read_timeout_ms",
            "ahci_bounce_kb",
        ] {
            assert!(
                Config::from_str(&replace_parameter(name, &format!("{name} = 0"))).is_err(),
                "{name} accepted zero"
            );
        }
        assert!(
            Config::from_str(&replace_parameter(
                "ahci_read_warn_ms",
                "ahci_read_warn_ms = 10000"
            ))
            .is_err()
        );
        assert!(
            Config::from_str(&replace_parameter(
                "ahci_bounce_kb",
                "ahci_bounce_kb = 4097"
            ))
            .is_err()
        );

        let mut config = Config::from_str(&content).unwrap();
        config.parameters.ahci_hba_reset_timeout_ms = None;
        config.parameters.ahci_engine_timeout_ms = None;
        config.parameters.ahci_port_timeout_ms = None;
        config.parameters.ahci_command_timeout_ms = None;
        config.parameters.ahci_read_warn_ms = None;
        config.parameters.ahci_read_timeout_ms = None;
        config.parameters.ahci_bounce_kb = None;
        let defaults = Config::from_str(&content).unwrap();
        config
            .parameters
            .materialize_defaults(Some(&defaults.parameters))
            .unwrap();
        let defs = config.parameters.gen_kconfig_defs();
        assert!(defs.contains("pub const AHCI_HBA_RESET_TIMEOUT_MS: u64 = 1000;"));
        assert!(defs.contains("pub const AHCI_READ_TIMEOUT_MS: u64 = 10000;"));
        assert!(defs.contains("pub const AHCI_BOUNCE_KB: usize = 128;"));
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
            Config::from_str(&replace_parameter("print_log_level", "print_log_level = 8")).is_err()
        );
        assert!(
            Config::from_str(&replace_parameter(
                "record_log_level",
                "record_log_level = 8"
            ))
            .is_err()
        );
    }
}
