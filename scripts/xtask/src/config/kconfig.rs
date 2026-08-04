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

#[derive(Deserialize, Debug, Serialize, Clone, Copy, PartialEq, Eq)]
pub enum TidAllocPolicy {
    #[serde(rename = "bitmap")]
    Bitmap,
    #[serde(rename = "oneshot")]
    OneShot,
}

impl TidAllocPolicy {
    fn kernel_variant(self) -> &'static str {
        match self {
            Self::Bitmap => "Bitmap",
            Self::OneShot => "OneShot",
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
    pub execve_max_string_count: Option<usize>,
    pub max_processes: Option<u64>,
    pub epoll_file_max_waiters: Option<usize>,
    pub max_iovec_count: Option<usize>,
    pub getdents64_buffer_bytes: Option<usize>,
    pub pipe_capacity_pages: Option<usize>,
    pub pipe_max_capacity_pages: Option<usize>,
    pub unix_stream_direction_capacity_bytes: Option<usize>,
    pub unix_listener_max_backlog: Option<usize>,
    pub unix_seqpacket_max_payload_bytes: Option<usize>,
    pub unix_seqpacket_direction_capacity_bytes: Option<usize>,
    pub unix_seqpacket_direction_max_records: Option<usize>,
    pub tid_alloc_policy: Option<TidAllocPolicy>,
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
    pub initial_umask: Option<u16>,
    pub ramdisk_count: Option<usize>,
    pub loop_device_count: Option<usize>,
    pub ns16550a_default_baud: Option<u32>,
    pub ns16550a_fallback_clock_hz: Option<u32>,
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
    pub virtio_net_queue_size: Option<usize>,
    pub virtio_net_frame_capacity_bytes: Option<usize>,
    pub net_pump_ingress_budget_frames: Option<usize>,
    pub net_pump_egress_budget_steps: Option<usize>,
    pub net_worker_repoll_rounds: Option<usize>,
    pub net_local_link_packet_capacity: Option<usize>,
    pub net_local_link_mtu_bytes: Option<usize>,
    pub net_udp_endpoint_capacity: Option<usize>,
    pub net_udp_tx_datagram_capacity: Option<usize>,
    pub net_udp_rx_datagram_capacity: Option<usize>,
    pub net_udp_max_payload_bytes: Option<usize>,
    pub net_udp_ephemeral_port_first: Option<u16>,
    pub net_udp_ephemeral_port_last: Option<u16>,
    pub net_icmp_raw_endpoint_capacity: Option<usize>,
    pub net_icmp_raw_tx_packet_capacity: Option<usize>,
    pub net_icmp_raw_tx_byte_capacity: Option<usize>,
    pub net_icmp_raw_rx_packet_capacity: Option<usize>,
    pub net_icmp_raw_rx_byte_capacity: Option<usize>,
    pub net_icmp_raw_default_ttl: Option<u8>,
    pub net_icmp_raw_default_tos: Option<u8>,
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
        materialize!(execve_max_string_count);
        materialize!(max_processes);
        materialize!(epoll_file_max_waiters);
        materialize!(max_iovec_count);
        materialize!(getdents64_buffer_bytes);
        materialize!(pipe_capacity_pages);
        materialize!(pipe_max_capacity_pages);
        materialize!(unix_stream_direction_capacity_bytes);
        materialize!(unix_listener_max_backlog);
        materialize!(unix_seqpacket_max_payload_bytes);
        materialize!(unix_seqpacket_direction_capacity_bytes);
        materialize!(unix_seqpacket_direction_max_records);
        materialize!(tid_alloc_policy);
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
        materialize!(initial_umask);
        materialize!(ramdisk_count);
        materialize!(loop_device_count);
        materialize!(ns16550a_default_baud);
        materialize!(ns16550a_fallback_clock_hz);
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
        materialize!(virtio_net_queue_size);
        materialize!(virtio_net_frame_capacity_bytes);
        materialize!(net_pump_ingress_budget_frames);
        materialize!(net_pump_egress_budget_steps);
        materialize!(net_worker_repoll_rounds);
        materialize!(net_local_link_packet_capacity);
        materialize!(net_local_link_mtu_bytes);
        materialize!(net_udp_endpoint_capacity);
        materialize!(net_udp_tx_datagram_capacity);
        materialize!(net_udp_rx_datagram_capacity);
        materialize!(net_udp_max_payload_bytes);
        materialize!(net_udp_ephemeral_port_first);
        materialize!(net_udp_ephemeral_port_last);
        materialize!(net_icmp_raw_endpoint_capacity);
        materialize!(net_icmp_raw_tx_packet_capacity);
        materialize!(net_icmp_raw_tx_byte_capacity);
        materialize!(net_icmp_raw_rx_packet_capacity);
        materialize!(net_icmp_raw_rx_byte_capacity);
        materialize!(net_icmp_raw_default_ttl);
        materialize!(net_icmp_raw_default_tos);
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
/// This value must not exceed `RECORD_LOG_LEVEL` because an unrecorded
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
/// Maximum number of strings accepted in each execve argv or envp vector.
pub const EXECVE_MAX_STRING_COUNT: usize = {};
/// Maximum number of processes
pub const MAX_PROCESSES: u64 = {};
/// Fixed waiter-route capacity per epoll instance.
pub const EPOLL_FILE_MAX_WAITERS: usize = {};
/// Maximum number of vectors imported by one ordinary vector I/O request.
pub const MAX_IOVEC_COUNT: usize = {};
/// Maximum kernel staging buffer used by one getdents64 call.
pub const GETDENTS64_BUFFER_BYTES: usize = {};
/// Default anonymous-pipe capacity in pages.
pub const PIPE_CAPACITY_PAGES: usize = {};
/// Maximum anonymous-pipe capacity in pages.
pub const PIPE_MAX_CAPACITY_PAGES: usize = {};
/// Fixed byte capacity of each AF_UNIX stream direction.
pub const UNIX_STREAM_DIRECTION_CAPACITY_BYTES: usize = {};
/// Maximum normalized listen backlog for AF_UNIX connection-oriented listeners.
pub const UNIX_LISTENER_MAX_BACKLOG: usize = {};
/// Maximum payload bytes in one AF_UNIX seqpacket record.
pub const UNIX_SEQPACKET_MAX_PAYLOAD_BYTES: usize = {};
/// Fixed byte capacity of each AF_UNIX seqpacket direction.
pub const UNIX_SEQPACKET_DIRECTION_CAPACITY_BYTES: usize = {};
/// Maximum committed records in each AF_UNIX seqpacket direction.
pub const UNIX_SEQPACKET_DIRECTION_MAX_RECORDS: usize = {};
/// Allocation policy for ordinary task IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TidAllocPolicy {{
    Bitmap,
    OneShot,
}}
/// Selected allocation policy for ordinary task IDs.
pub const TID_ALLOC_POLICY: TidAllocPolicy = TidAllocPolicy::{};
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
/// Build-time file-table capacity and system-wide fd-number ceiling.
/// Runtime rlimit syscalls change only the owning process policy.
pub const MAX_FD_PER_PROCESS: usize = {};
/// Initial file creation mask for user filesystem contexts.
pub const INITIAL_UMASK: u16 = {};
/// Number of static ramdisk block devices to publish at boot.
pub const RAMDISK_COUNT: usize = {};
/// Number of static loop block devices to publish at boot.
pub const LOOP_DEVICE_COUNT: usize = {};
/// Default NS16550A baud used when stdout-path has no device-specific options.
pub const NS16550A_DEFAULT_BAUD: u32 = {};
/// NS16550A input clock used when firmware omits clock-frequency.
pub const NS16550A_FALLBACK_CLOCK_HZ: u32 = {};
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
/// Descriptor capacity of each VirtIO-Net queue.
pub const VIRTIO_NET_QUEUE_SIZE: usize = {};
/// Bytes owned by each VirtIO-Net frame backing, including the VirtIO header.
pub const VIRTIO_NET_FRAME_CAPACITY_BYTES: usize = {};
/// Maximum ingress frames advanced by one stack pump.
pub const NET_PUMP_INGRESS_BUDGET_FRAMES: usize = {};
/// Maximum egress steps advanced by one stack pump.
pub const NET_PUMP_EGRESS_BUDGET_STEPS: usize = {};
/// Maximum immediate repoll rounds before a network worker yields.
pub const NET_WORKER_REPOLL_ROUNDS: usize = {};
/// Shared packet-slot capacity of the production local software link.
pub const NET_LOCAL_LINK_PACKET_CAPACITY: usize = {};
/// Maximum IP-medium packet size of the production local software link.
pub const NET_LOCAL_LINK_MTU_BYTES: usize = {};
/// Maximum number of live UDP endpoints in the initial domain.
pub const NET_UDP_ENDPOINT_CAPACITY: usize = {};
/// Per-endpoint UDP transmit datagram capacity.
pub const NET_UDP_TX_DATAGRAM_CAPACITY: usize = {};
/// Per-endpoint UDP receive datagram capacity.
pub const NET_UDP_RX_DATAGRAM_CAPACITY: usize = {};
/// Maximum UDP payload bytes reserved by one protocol engine datagram.
pub const NET_UDP_MAX_PAYLOAD_BYTES: usize = {};
/// First port in the deterministic UDP ephemeral allocation range.
pub const NET_UDP_EPHEMERAL_PORT_FIRST: u16 = {};
/// Last port in the deterministic UDP ephemeral allocation range.
pub const NET_UDP_EPHEMERAL_PORT_LAST: u16 = {};
/// Maximum live IPv4 ICMP raw endpoints in the initial domain.
pub const NET_ICMP_RAW_ENDPOINT_CAPACITY: usize = {};
/// Per-endpoint committed ICMP raw transmit packet slots.
pub const NET_ICMP_RAW_TX_PACKET_CAPACITY: usize = {};
/// Per-endpoint committed ICMP raw transmit packet bytes.
pub const NET_ICMP_RAW_TX_BYTE_CAPACITY: usize = {};
/// Per-endpoint detached ICMP raw receive packet slots.
pub const NET_ICMP_RAW_RX_PACKET_CAPACITY: usize = {};
/// Per-endpoint detached ICMP raw receive packet bytes.
pub const NET_ICMP_RAW_RX_BYTE_CAPACITY: usize = {};
/// Default IPv4 TTL for ICMP raw Socket sends.
pub const NET_ICMP_RAW_DEFAULT_TTL: u8 = {};
/// Default IPv4 TOS for ICMP raw Socket sends.
pub const NET_ICMP_RAW_DEFAULT_TOS: u8 = {};
"#,
            resolved!(bootstrap_heap_shift_kb),
            resolved!(log_buffer_shift_kb),
            resolved!(log_record_shift_bytes),
            resolved!(print_log_level),
            resolved!(record_log_level),
            resolved!(kstack_shift_kb),
            resolved!(remap_shift_gb),
            resolved!(max_logical_cpus),
            resolved!(max_ident_len_bytes),
            resolved!(max_path_len_bytes),
            resolved!(execve_max_string_count),
            resolved!(max_processes),
            resolved!(epoll_file_max_waiters),
            resolved!(max_iovec_count),
            resolved!(getdents64_buffer_bytes),
            resolved!(pipe_capacity_pages),
            resolved!(pipe_max_capacity_pages),
            resolved!(unix_stream_direction_capacity_bytes),
            resolved!(unix_listener_max_backlog),
            resolved!(unix_seqpacket_max_payload_bytes),
            resolved!(unix_seqpacket_direction_capacity_bytes),
            resolved!(unix_seqpacket_direction_max_records),
            resolved!(tid_alloc_policy).kernel_variant(),
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
            resolved!(initial_umask),
            resolved!(ramdisk_count),
            resolved!(loop_device_count),
            resolved!(ns16550a_default_baud),
            resolved!(ns16550a_fallback_clock_hz),
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
            resolved!(virtio_net_queue_size),
            resolved!(virtio_net_frame_capacity_bytes),
            resolved!(net_pump_ingress_budget_frames),
            resolved!(net_pump_egress_budget_steps),
            resolved!(net_worker_repoll_rounds),
            resolved!(net_local_link_packet_capacity),
            resolved!(net_local_link_mtu_bytes),
            resolved!(net_udp_endpoint_capacity),
            resolved!(net_udp_tx_datagram_capacity),
            resolved!(net_udp_rx_datagram_capacity),
            resolved!(net_udp_max_payload_bytes),
            resolved!(net_udp_ephemeral_port_first),
            resolved!(net_udp_ephemeral_port_last),
            resolved!(net_icmp_raw_endpoint_capacity),
            resolved!(net_icmp_raw_tx_packet_capacity),
            resolved!(net_icmp_raw_tx_byte_capacity),
            resolved!(net_icmp_raw_rx_packet_capacity),
            resolved!(net_icmp_raw_rx_byte_capacity),
            resolved!(net_icmp_raw_default_ttl),
            resolved!(net_icmp_raw_default_tos),
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
        Ok(toml::from_str(content)?)
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

    fn defaults() -> Parameters {
        Config::from_str(include_str!("../../../../conf/.defconfig"))
            .unwrap()
            .parameters
    }

    #[test]
    fn udp_defaults_materialize_and_generate_exact_constants() {
        let mut parameters = defaults();
        parameters.materialize_defaults(None).unwrap();
        let generated = parameters.gen_kconfig_defs();
        for expected in [
            "pub const NET_UDP_ENDPOINT_CAPACITY: usize = 64;",
            "pub const NET_UDP_TX_DATAGRAM_CAPACITY: usize = 8;",
            "pub const NET_UDP_RX_DATAGRAM_CAPACITY: usize = 64;",
            "pub const NET_UDP_MAX_PAYLOAD_BYTES: usize = 1472;",
            "pub const NET_UDP_EPHEMERAL_PORT_FIRST: u16 = 32768;",
            "pub const NET_UDP_EPHEMERAL_PORT_LAST: u16 = 60999;",
        ] {
            assert!(
                generated.contains(expected),
                "missing generated constant {expected}"
            );
        }
    }

    #[test]
    fn icmp_raw_defaults_materialize_and_generate_exact_constants() {
        let mut parameters = defaults();
        parameters.materialize_defaults(None).unwrap();
        let generated = parameters.gen_kconfig_defs();
        for expected in [
            "pub const NET_ICMP_RAW_ENDPOINT_CAPACITY: usize = 64;",
            "pub const NET_ICMP_RAW_TX_PACKET_CAPACITY: usize = 8;",
            "pub const NET_ICMP_RAW_TX_BYTE_CAPACITY: usize = 65536;",
            "pub const NET_ICMP_RAW_RX_PACKET_CAPACITY: usize = 64;",
            "pub const NET_ICMP_RAW_RX_BYTE_CAPACITY: usize = 262144;",
            "pub const NET_ICMP_RAW_DEFAULT_TTL: u8 = 64;",
            "pub const NET_ICMP_RAW_DEFAULT_TOS: u8 = 0;",
        ] {
            assert!(
                generated.contains(expected),
                "missing generated constant {expected}"
            );
        }
    }

    #[test]
    fn getdents64_buffer_default_materializes_and_generates() {
        let mut parameters = defaults();
        parameters.materialize_defaults(None).unwrap();
        assert!(
            parameters
                .gen_kconfig_defs()
                .contains("pub const GETDENTS64_BUFFER_BYTES: usize = 2097152;")
        );
    }

    #[test]
    fn max_iovec_count_default_materializes_and_generates() {
        let mut parameters = defaults();
        parameters.materialize_defaults(None).unwrap();
        assert_eq!(parameters.max_iovec_count, Some(1024));
        assert!(
            parameters
                .gen_kconfig_defs()
                .contains("pub const MAX_IOVEC_COUNT: usize = 1024;")
        );
    }

    #[test]
    fn reduced_max_iovec_count_materializes() {
        let mut parameters = defaults();
        parameters.max_iovec_count = Some(16);
        parameters.materialize_defaults(None).unwrap();
        assert!(
            parameters
                .gen_kconfig_defs()
                .contains("pub const MAX_IOVEC_COUNT: usize = 16;")
        );
    }

    #[test]
    fn execve_string_count_default_materializes_and_generates() {
        let mut parameters = defaults();
        parameters.materialize_defaults(None).unwrap();
        assert!(
            parameters
                .gen_kconfig_defs()
                .contains("pub const EXECVE_MAX_STRING_COUNT: usize = 256;")
        );
    }

    #[test]
    fn pipe_capacity_semantics_are_deferred_to_kernel_compilation() {
        let mut parameters = defaults();
        parameters.materialize_defaults(None).unwrap();
        let generated = parameters.gen_kconfig_defs();
        assert!(generated.contains("pub const PIPE_CAPACITY_PAGES: usize = 2;"));
        assert!(generated.contains("pub const PIPE_MAX_CAPACITY_PAGES: usize = 16;"));

        parameters.pipe_capacity_pages = Some(3);
        parameters.pipe_max_capacity_pages = Some(1);
        let generated = parameters.gen_kconfig_defs();
        assert!(generated.contains("pub const PIPE_CAPACITY_PAGES: usize = 3;"));
        assert!(generated.contains("pub const PIPE_MAX_CAPACITY_PAGES: usize = 1;"));
    }

    #[test]
    fn unix_stream_capacity_default_materializes_and_generates() {
        let mut parameters = defaults();
        parameters.materialize_defaults(None).unwrap();
        assert!(
            parameters
                .gen_kconfig_defs()
                .contains("pub const UNIX_STREAM_DIRECTION_CAPACITY_BYTES: usize = 65536;")
        );
        assert!(
            parameters
                .gen_kconfig_defs()
                .contains("pub const UNIX_LISTENER_MAX_BACKLOG: usize = 128;")
        );
    }

    #[test]
    fn udp_parameter_semantics_are_deferred_to_kernel_compilation() {
        let mut parameters = defaults();
        parameters.net_udp_endpoint_capacity = Some(0);
        parameters.net_udp_tx_datagram_capacity = Some(usize::MAX);
        parameters.net_udp_max_payload_bytes = Some(1);
        parameters.net_udp_ephemeral_port_first = Some(60_000);
        parameters.net_udp_ephemeral_port_last = Some(50_000);
        parameters.materialize_defaults(None).unwrap();

        let generated = parameters.gen_kconfig_defs();
        for expected in [
            "pub const NET_UDP_ENDPOINT_CAPACITY: usize = 0;".to_string(),
            format!(
                "pub const NET_UDP_TX_DATAGRAM_CAPACITY: usize = {};",
                usize::MAX
            ),
            "pub const NET_UDP_MAX_PAYLOAD_BYTES: usize = 1;".to_string(),
            "pub const NET_UDP_EPHEMERAL_PORT_FIRST: u16 = 60000;".to_string(),
            "pub const NET_UDP_EPHEMERAL_PORT_LAST: u16 = 50000;".to_string(),
        ] {
            assert!(
                generated.contains(&expected),
                "missing generated constant {expected}"
            );
        }
    }
}
