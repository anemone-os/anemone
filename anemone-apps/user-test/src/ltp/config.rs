//! Static LTP registration and runner policy.
//!
//! This module is deliberately not a runtime configuration layer. The profile
//! file selects groups, while roots, env, disabled cases, and containment
//! timing remain compile-time runner facts so cleanup work cannot silently
//! change the competition-visible execution surface.

use super::time::MICROS_PER_SECOND;

const LTP_CASE_TIMEOUT_SECONDS: i64 = 60;
const LTP_CASE_KILL_GRACE_SECONDS: i64 = 5;
const LTP_HEARTBEAT_PRINT_INTERVAL_SECONDS: i64 = 5;
const LTP_HEARTBEAT_SLEEP_TICK_SECONDS: i64 = 1;
const LTP_HEARTBEAT_STOP_GRACE_SECONDS: i64 = 2;
const LTP_WAIT_LOOP_PROBE_INTERVAL_SECONDS: i64 = 5;
const LTP_OUTPUT_FILTER_STOP_GRACE_SECONDS: i64 = 1;

const GLIBC_LTP_ENV: &[&str] = &[
    "PATH=/glibc/ltp/testcases/bin:/glibc/bin:/glibc/usr/bin:/bin:/usr/bin:/sbin:/usr/sbin",
    "LTPROOT=/glibc/ltp",
    "LTP_VIRT_OVERRIDE=kvm",
    "LTP_COLORIZE_OUTPUT=1",
    "TMPDIR=/tmp",
    "KCONFIG_PATH=/etc/ltp/anemone-kconfig",
];

const MUSL_LTP_ENV: &[&str] = &[
    "PATH=/musl/ltp/testcases/bin:/musl/bin:/musl/usr/bin:/bin:/usr/bin:/sbin:/usr/sbin",
    "LTPROOT=/musl/ltp",
    "LTP_VIRT_OVERRIDE=kvm",
    "LTP_COLORIZE_OUTPUT=1",
    "TMPDIR=/tmp",
    "KCONFIG_PATH=/etc/ltp/anemone-kconfig",
];

pub(super) struct LtpRunPolicy {
    // All fields describe runner-owned containment and diagnostics timing. They
    // are typed to prevent call sites from mixing seconds and microseconds, but
    // they are not user-tunable knobs.
    pub(super) case_timeout_us: i64,
    pub(super) case_timeout_seconds: i64,
    pub(super) kill_grace_us: i64,
    pub(super) kill_grace_seconds: i64,
    pub(super) heartbeat_print_interval_us: i64,
    pub(super) heartbeat_sleep_tick_seconds: i64,
    pub(super) heartbeat_stop_grace_us: i64,
    pub(super) heartbeat_stop_grace_seconds: i64,
    pub(super) wait_loop_probe_interval_us: i64,
    pub(super) output_filter_stop_grace_us: i64,
    pub(super) output_filter_stop_grace_seconds: i64,
}

impl LtpRunPolicy {
    pub(super) const DEFAULT: Self = Self {
        // Temporary containment for ANE-20260616-LTP-POST-SUMMARY-HANG: keep
        // long profiles moving while the kernel-side wait/cleanup root cause is
        // open. A timeout is still an infra failure, not testcase success and
        // not a long-term kernel watchdog policy.
        case_timeout_us: LTP_CASE_TIMEOUT_SECONDS * MICROS_PER_SECOND,
        case_timeout_seconds: LTP_CASE_TIMEOUT_SECONDS,
        kill_grace_us: LTP_CASE_KILL_GRACE_SECONDS * MICROS_PER_SECOND,
        kill_grace_seconds: LTP_CASE_KILL_GRACE_SECONDS,
        heartbeat_print_interval_us: LTP_HEARTBEAT_PRINT_INTERVAL_SECONDS * MICROS_PER_SECOND,
        heartbeat_sleep_tick_seconds: LTP_HEARTBEAT_SLEEP_TICK_SECONDS,
        heartbeat_stop_grace_us: LTP_HEARTBEAT_STOP_GRACE_SECONDS * MICROS_PER_SECOND,
        heartbeat_stop_grace_seconds: LTP_HEARTBEAT_STOP_GRACE_SECONDS,
        wait_loop_probe_interval_us: LTP_WAIT_LOOP_PROBE_INTERVAL_SECONDS * MICROS_PER_SECOND,
        output_filter_stop_grace_us: LTP_OUTPUT_FILTER_STOP_GRACE_SECONDS * MICROS_PER_SECOND,
        output_filter_stop_grace_seconds: LTP_OUTPUT_FILTER_STOP_GRACE_SECONDS,
    };
}

pub(super) struct LtpRoot {
    pub(super) family: &'static str,
    pub(super) label: &'static str,
    pub(super) workdir: &'static str,
    pub(super) envp: &'static [&'static str],
    pub(super) disabled_cases: &'static [&'static str],
}

pub(super) const LTP_ROOTS: &[LtpRoot] = &[
    LtpRoot {
        family: "glibc",
        label: "ltp-glibc",
        workdir: "/glibc",
        envp: GLIBC_LTP_ENV,
        disabled_cases: &[],
    },
    LtpRoot {
        family: "musl",
        label: "ltp-musl",
        workdir: "/musl",
        envp: MUSL_LTP_ENV,
        disabled_cases: &["sbrk01"],
    },
];

pub(super) struct LtpGroup {
    pub(super) name: &'static str,
    pub(super) cases: &'static str,
}

pub(super) const LTP_GROUPS: &[LtpGroup] = &[
    LtpGroup {
        name: "chmod",
        cases: include_str!("../../ltp/groups/chmod.txt"),
    },
    LtpGroup {
        name: "chown",
        cases: include_str!("../../ltp/groups/chown.txt"),
    },
    LtpGroup {
        name: "clone",
        cases: include_str!("../../ltp/groups/clone.txt"),
    },
    LtpGroup {
        name: "clock",
        cases: include_str!("../../ltp/groups/clock.txt"),
    },
    LtpGroup {
        name: "credentials",
        cases: include_str!("../../ltp/groups/credentials.txt"),
    },
    LtpGroup {
        name: "eventfd",
        cases: include_str!("../../ltp/groups/eventfd.txt"),
    },
    LtpGroup {
        name: "exec",
        cases: include_str!("../../ltp/groups/exec.txt"),
    },
    LtpGroup {
        name: "fanotify",
        cases: include_str!("../../ltp/groups/fanotify.txt"),
    },
    LtpGroup {
        name: "fcntl",
        cases: include_str!("../../ltp/groups/fcntl.txt"),
    },
    LtpGroup {
        name: "fd",
        cases: include_str!("../../ltp/groups/fd.txt"),
    },
    LtpGroup {
        name: "fs",
        cases: include_str!("../../ltp/groups/fs.txt"),
    },
    // LtpGroup {
    //     name: "full",
    //     cases: include_str!("../../ltp/groups/full.txt"),
    // },
    LtpGroup {
        name: "futex",
        cases: include_str!("../../ltp/groups/futex.txt"),
    },
    LtpGroup {
        name: "ioctl",
        cases: include_str!("../../ltp/groups/ioctl.txt"),
    },
    LtpGroup {
        name: "iomux",
        cases: include_str!("../../ltp/groups/iomux.txt"),
    },
    LtpGroup {
        name: "ipc",
        cases: include_str!("../../ltp/groups/ipc.txt"),
    },
    LtpGroup {
        name: "memory",
        cases: include_str!("../../ltp/groups/memory.txt"),
    },
    LtpGroup {
        name: "mount-legacy",
        cases: include_str!("../../ltp/groups/mount-legacy.txt"),
    },
    LtpGroup {
        name: "open",
        cases: include_str!("../../ltp/groups/open.txt"),
    },
    LtpGroup {
        name: "pipe",
        cases: include_str!("../../ltp/groups/pipe.txt"),
    },
    LtpGroup {
        name: "read-write",
        cases: include_str!("../../ltp/groups/read-write.txt"),
    },
    LtpGroup {
        name: "schedule",
        cases: include_str!("../../ltp/groups/schedule.txt"),
    },
    LtpGroup {
        name: "sendfile",
        cases: include_str!("../../ltp/groups/sendfile.txt"),
    },
    LtpGroup {
        name: "signal",
        cases: include_str!("../../ltp/groups/signal.txt"),
    },
    LtpGroup {
        name: "splice",
        cases: include_str!("../../ltp/groups/splice.txt"),
    },
    LtpGroup {
        name: "sys",
        cases: include_str!("../../ltp/groups/sys.txt"),
    },
    LtpGroup {
        name: "timer",
        cases: include_str!("../../ltp/groups/timer.txt"),
    },
    LtpGroup {
        name: "timerfd",
        cases: include_str!("../../ltp/groups/timerfd.txt"),
    },
    LtpGroup {
        name: "tmp",
        cases: include_str!("../../ltp/groups/tmp.txt"),
    },
    LtpGroup {
        name: "wait",
        cases: include_str!("../../ltp/groups/wait.txt"),
    },
];
