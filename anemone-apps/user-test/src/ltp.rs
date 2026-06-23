use anemone_rs::{
    abi::time::linux::TimeSpec,
    os::linux::{
        fs::{chdir, close, fstatat, pipe2, read, write, AtFd, Fd, PipeFlags},
        process::{
            execve, exit, fork, sched_yield, setpgid,
            signal::{kill, SigNo},
            wait4, WStatus, WStatusRaw, WaitFor, WaitOptions,
        },
        time::{gettimeofday, nanosleep},
    },
    prelude::*,
};

const ACTIVE_PROFILE: &str = include_str!("../ltp/profile.txt");
const ETC_PASSWD: &str = include_str!("../fixtures/passwd");
const ETC_GROUP: &str = include_str!("../fixtures/group");
const LTP_KCONFIG: &str = include_str!("../fixtures/ltp-kconfig");
const LTP_MODULES_BUILTIN: &str = include_str!("../fixtures/modules.builtin");
const LTP_MODULES_DEP: &str = include_str!("../fixtures/modules.dep");

const MICROS_PER_SECOND: i64 = 1_000_000;
// Temporary containment for ANE-20260616-LTP-POST-SUMMARY-HANG: keep long
// profiles moving while the kernel-side wait/cleanup root cause is open.
const LTP_CASE_TIMEOUT_SECONDS: i64 = 60;
const LTP_CASE_KILL_GRACE_SECONDS: i64 = 5;
const LTP_CASE_TIMEOUT_EXIT_CODE: i32 = 124;
const LTP_HEARTBEAT_PRINT_INTERVAL_SECONDS: i64 = 5;
const LTP_HEARTBEAT_SLEEP_TICK_SECONDS: i64 = 1;
const LTP_HEARTBEAT_STOP_GRACE_SECONDS: i64 = 2;
const LTP_WAIT_LOOP_PROBE_INTERVAL_SECONDS: i64 = 5;
// LTP result bits use TCONF=32 for "unsupported configuration". Only a pure
// TCONF exit is a skip; mixed TCONF|TFAIL/TBROK still represents a real
// failure.
const LTP_TCONF_EXIT_CODE: i32 = 32;
const LTP_CASE_TIMEOUT_US: i64 = LTP_CASE_TIMEOUT_SECONDS * MICROS_PER_SECOND;
const LTP_CASE_KILL_GRACE_US: i64 = LTP_CASE_KILL_GRACE_SECONDS * MICROS_PER_SECOND;
const LTP_HEARTBEAT_PRINT_INTERVAL_US: i64 =
    LTP_HEARTBEAT_PRINT_INTERVAL_SECONDS * MICROS_PER_SECOND;
const LTP_HEARTBEAT_STOP_GRACE_US: i64 = LTP_HEARTBEAT_STOP_GRACE_SECONDS * MICROS_PER_SECOND;
const LTP_WAIT_LOOP_PROBE_INTERVAL_US: i64 =
    LTP_WAIT_LOOP_PROBE_INTERVAL_SECONDS * MICROS_PER_SECOND;

const GLIBC_LTP_ENV: &[&str] = &[
    "PATH=/glibc/ltp/testcases/bin:/glibc/bin:/glibc/usr/bin:/bin:/usr/bin:/sbin:/usr/sbin",
    "LTPROOT=/glibc/ltp",
    "LTP_VIRT_OVERRIDE=kvm",
    // "LTP_COLORIZE_OUTPUT=1",
    "ANSI_COLOR=0",
    "TMPDIR=/tmp",
    "KCONFIG_PATH=/etc/ltp/anemone-kconfig",
];

const MUSL_LTP_ENV: &[&str] = &[
    "PATH=/musl/ltp/testcases/bin:/musl/bin:/musl/usr/bin:/bin:/usr/bin:/sbin:/usr/sbin",
    "LTPROOT=/musl/ltp",
    "LTP_VIRT_OVERRIDE=kvm",
    // "LTP_COLORIZE_OUTPUT=1",
    "ANSI_COLOR=0",
    "TMPDIR=/tmp",
    "KCONFIG_PATH=/etc/ltp/anemone-kconfig",
];

struct LtpRoot {
    family: &'static str,
    label: &'static str,
    workdir: &'static str,
    envp: &'static [&'static str],
    disabled_cases: &'static [&'static str],
}

const LTP_ROOTS: &[LtpRoot] = &[
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

struct LtpGroup {
    name: &'static str,
    cases: &'static str,
}

const LTP_GROUPS: &[LtpGroup] = &[
    LtpGroup {
        name: "clone",
        cases: include_str!("../ltp/groups/clone.txt"),
    },
    LtpGroup {
        name: "exec",
        cases: include_str!("../ltp/groups/exec.txt"),
    },
    LtpGroup {
        name: "eventfd",
        cases: include_str!("../ltp/groups/eventfd.txt"),
    },
    LtpGroup {
        name: "timerfd",
        cases: include_str!("../ltp/groups/timerfd.txt"),
    },
    LtpGroup {
        name: "chmod",
        cases: include_str!("../ltp/groups/chmod.txt"),
    },
    LtpGroup {
        name: "chown",
        cases: include_str!("../ltp/groups/chown.txt"),
    },
    LtpGroup {
        name: "fanotify",
        cases: include_str!("../ltp/groups/fanotify.txt"),
    },
    LtpGroup {
        name: "fcntl",
        cases: include_str!("../ltp/groups/fcntl.txt"),
    },
    LtpGroup {
        name: "fd",
        cases: include_str!("../ltp/groups/fd.txt"),
    },
    LtpGroup {
        name: "fs",
        cases: include_str!("../ltp/groups/fs.txt"),
    },
    LtpGroup {
        name: "mount-legacy",
        cases: include_str!("../ltp/groups/mount-legacy.txt"),
    },
    // LtpGroup {
    //     name: "full",
    //     cases: include_str!("../ltp/groups/full.txt"),
    // },
    LtpGroup {
        name: "futex",
        cases: include_str!("../ltp/groups/futex.txt"),
    },
    LtpGroup {
        name: "ioctl",
        cases: include_str!("../ltp/groups/ioctl.txt"),
    },
    LtpGroup {
        name: "memory",
        cases: include_str!("../ltp/groups/memory.txt"),
    },
    LtpGroup {
        name: "open",
        cases: include_str!("../ltp/groups/open.txt"),
    },
    LtpGroup {
        name: "pipe",
        cases: include_str!("../ltp/groups/pipe.txt"),
    },
    LtpGroup {
        name: "read-write",
        cases: include_str!("../ltp/groups/read-write.txt"),
    },
    LtpGroup {
        name: "ipc",
        cases: include_str!("../ltp/groups/ipc.txt"),
    },
    LtpGroup {
        name: "tmp",
        cases: include_str!("../ltp/groups/tmp.txt"),
    },
    LtpGroup {
        name: "wait",
        cases: include_str!("../ltp/groups/wait.txt"),
    },
    LtpGroup {
        name: "credentials",
        cases: include_str!("../ltp/groups/credentials.txt"),
    },
    LtpGroup {
        name: "sendfile",
        cases: include_str!("../ltp/groups/sendfile.txt"),
    },
    LtpGroup {
        name: "signal",
        cases: include_str!("../ltp/groups/signal.txt"),
    },
    LtpGroup {
        name: "splice",
        cases: include_str!("../ltp/groups/splice.txt"),
    },
    LtpGroup {
        name: "schedule",
        cases: include_str!("../ltp/groups/schedule.txt"),
    },
    LtpGroup {
        name: "iomux",
        cases: include_str!("../ltp/groups/iomux.txt"),
    },
    LtpGroup {
        name: "syscalls",
        cases: include_str!("../ltp/groups/syscalls.txt"),
    },
];

struct LtpFixture {
    path: &'static str,
    content: &'static str,
}

const LTP_FIXTURES: &[LtpFixture] = &[
    LtpFixture {
        path: "/etc/passwd",
        content: ETC_PASSWD,
    },
    LtpFixture {
        path: "/etc/group",
        content: ETC_GROUP,
    },
    LtpFixture {
        path: "/etc/ltp/anemone-kconfig",
        content: LTP_KCONFIG,
    },
    // rv64 switches /lib between runtime lib directories before running LTP.
    // Keep the module metadata visible through that active /lib symlink.
    LtpFixture {
        path: "/glibc/lib/modules/6.6.32/modules.dep",
        content: LTP_MODULES_DEP,
    },
    LtpFixture {
        path: "/glibc/lib/modules/6.6.32/modules.builtin",
        content: LTP_MODULES_BUILTIN,
    },
    LtpFixture {
        path: "/musl/lib/modules/6.6.32/modules.dep",
        content: LTP_MODULES_DEP,
    },
    LtpFixture {
        path: "/musl/lib/modules/6.6.32/modules.builtin",
        content: LTP_MODULES_BUILTIN,
    },
    LtpFixture {
        path: "/lib/modules/6.6.32/modules.dep",
        content: LTP_MODULES_DEP,
    },
    LtpFixture {
        path: "/lib/modules/6.6.32/modules.builtin",
        content: LTP_MODULES_BUILTIN,
    },
];

struct LtpCaseSpec<'a> {
    name: &'a str,
    executable: &'a str,
    args: Vec<&'a str>,
}

#[derive(Clone, Copy, Default)]
struct LtpSummary {
    attempted: usize,
    passed: usize,
    failed: usize,
    infra_failed: usize,
    skipped: usize,
}

impl LtpSummary {
    fn merge(&mut self, other: Self) {
        self.attempted += other.attempted;
        self.passed += other.passed;
        self.failed += other.failed;
        self.infra_failed += other.infra_failed;
        self.skipped += other.skipped;
    }
}

enum LtpCaseOutcome {
    Passed,
    Failed,
    InfraFailed,
    Skipped,
}

enum LtpCaseWaitResult {
    Exited(WStatus),
    TimedOut,
}

struct LtpHeartbeat {
    child: Option<u32>,
    write_fd: Option<Fd>,
    snapshot_seq: u64,
}

impl LtpHeartbeat {
    fn start_or_disabled() -> Self {
        match Self::start() {
            Ok(heartbeat) => heartbeat,
            Err(errno) => {
                println!("user-test: LTP heartbeat disabled: {errno:?}");
                Self::disabled()
            },
        }
    }

    fn start() -> Result<Self, Errno> {
        let (read_fd, write_fd) = pipe2(PipeFlags::CLOEXEC | PipeFlags::NONBLOCK)?;
        // The heartbeat channel is diagnostic-only. CLOEXEC keeps the long-lived
        // LTP case image from inheriting the writer and hiding parent shutdown.
        let fork_result = match fork() {
            Ok(result) => result,
            Err(errno) => {
                let _ = close(read_fd);
                let _ = close(write_fd);
                return Err(errno);
            },
        };

        match fork_result {
            Some(child) => {
                let _ = close(read_fd);
                let mut heartbeat = Self {
                    child: Some(child),
                    write_fd: Some(write_fd),
                    snapshot_seq: 0,
                };
                heartbeat.publish("started", "-", "-", "-", 0);
                Ok(heartbeat)
            },
            None => {
                let _ = close(write_fd);
                run_ltp_heartbeat_child(read_fd);
            },
        }
    }

    fn disabled() -> Self {
        Self {
            child: None,
            write_fd: None,
            snapshot_seq: 0,
        }
    }

    fn publish(&mut self, phase: &str, root: &str, group: &str, case: &str, case_pgrp: u32) {
        let Some(fd) = self.write_fd else {
            return;
        };

        self.snapshot_seq += 1;
        let now = now_us().unwrap_or(-1);
        let message = format!(
            "snapshot_seq={} now_us={} phase={} root={} group={} case={} case_pgrp={}\n",
            self.snapshot_seq, now, phase, root, group, case, case_pgrp,
        );

        match write(fd, message.as_bytes()) {
            Ok(_) | Err(EAGAIN) | Err(EINTR) => {},
            Err(EPIPE) => {
                println!("user-test: LTP heartbeat pipe closed; disabling updates");
                self.close_write_fd();
            },
            Err(errno) => {
                println!("user-test: LTP heartbeat update failed: {errno:?}");
                self.close_write_fd();
            },
        }
    }

    fn finish(mut self) {
        self.publish("finished", "-", "-", "-", 0);
        self.close_write_fd();
        self.wait_child_exit();
    }

    fn close_write_fd(&mut self) {
        if let Some(fd) = self.write_fd.take() {
            let _ = close(fd);
        }
    }

    fn wait_child_exit(&mut self) {
        let Some(child) = self.child.take() else {
            return;
        };

        let start_us = now_us().unwrap_or(0);
        let mut killed = false;
        loop {
            let mut wstatus = WStatusRaw::EMPTY;
            match wait4(
                WaitFor::ChildWithTgid(child),
                Some(&mut wstatus),
                WaitOptions::NOHANG,
            ) {
                Ok(Some(_)) | Err(ECHILD) => return,
                Ok(None) | Err(EINTR) => {},
                Err(errno) => {
                    println!("user-test: LTP heartbeat wait failed: {errno:?}");
                    return;
                },
            }

            let elapsed_us = elapsed_us_since(start_us).unwrap_or(LTP_HEARTBEAT_STOP_GRACE_US);
            if !killed && elapsed_us >= LTP_HEARTBEAT_STOP_GRACE_US {
                println!("user-test: LTP heartbeat did not exit after control pipe close; killing");
                let _ = kill(child as i32, SigNo::SIGKILL);
                killed = true;
            }
            if killed && elapsed_us >= LTP_HEARTBEAT_STOP_GRACE_US + MICROS_PER_SECOND {
                println!("user-test: LTP heartbeat child not reaped after kill; continuing");
                return;
            }

            let _ = sched_yield();
        }
    }
}

impl Drop for LtpHeartbeat {
    fn drop(&mut self) {
        self.close_write_fd();
    }
}

struct LtpWaitLoopProbe<'a> {
    root_label: &'a str,
    group_name: &'a str,
    case_name: &'a str,
    tid: u32,
    phase: &'static str,
    seq: u64,
    last_now_us: i64,
    next_probe_us: i64,
    log_iteration: bool,
}

impl<'a> LtpWaitLoopProbe<'a> {
    fn new(root_label: &'a str, group_name: &'a str, case_name: &'a str, tid: u32) -> Self {
        Self {
            root_label,
            group_name,
            case_name,
            tid,
            phase: "wait",
            seq: 0,
            last_now_us: 0,
            next_probe_us: 0,
            log_iteration: true,
        }
    }

    fn set_phase(&mut self, phase: &'static str) {
        self.phase = phase;
        self.next_probe_us = 0;
        self.log_iteration = true;
    }

    fn begin_iteration(&mut self) {
        self.log_iteration = self.next_probe_us == 0 || self.last_now_us >= self.next_probe_us;
        if self.log_iteration {
            self.seq += 1;
        }
    }

    fn finish_iteration(&mut self) {
        if !self.log_iteration {
            return;
        }

        let mut next = if self.next_probe_us == 0 {
            self.last_now_us
                .saturating_add(LTP_WAIT_LOOP_PROBE_INTERVAL_US)
        } else {
            self.next_probe_us
        };
        while self.last_now_us >= next {
            next = next.saturating_add(LTP_WAIT_LOOP_PROBE_INTERVAL_US);
        }
        self.next_probe_us = next;
        self.log_iteration = false;
    }

    fn before(&self, op: &str) {
        if self.log_iteration {
            println!(
                "user-test: LTP wait-loop probe seq={} phase={} root={} group={} case={} case_pgrp={} before {}",
                self.seq,
                self.phase,
                self.root_label,
                self.group_name,
                self.case_name,
                self.tid,
                op,
            );
        }
    }

    fn after(&self, op: &str, detail: core::fmt::Arguments<'_>) {
        if self.log_iteration {
            println!(
                "user-test: LTP wait-loop probe seq={} phase={} root={} group={} case={} case_pgrp={} after {} {}",
                self.seq,
                self.phase,
                self.root_label,
                self.group_name,
                self.case_name,
                self.tid,
                op,
                detail,
            );
        }
    }

    fn observe_now(&mut self, now_us: i64) {
        self.last_now_us = now_us;
    }
}

enum LtpHeartbeatPipe {
    Open,
    Closed,
}

fn run_ltp_heartbeat_child(read_fd: Fd) -> ! {
    let mut heartbeat_seq = 0u64;
    let mut snapshot =
        String::from("snapshot_seq=0 now_us=-1 phase=starting root=- group=- case=- case_pgrp=0");
    let mut pending = String::new();
    let mut last_snapshot_us = now_us().unwrap_or(0);
    let mut next_print_us = last_snapshot_us;

    loop {
        match drain_ltp_heartbeat_pipe(read_fd, &mut snapshot, &mut pending, &mut last_snapshot_us)
        {
            LtpHeartbeatPipe::Open => {},
            LtpHeartbeatPipe::Closed => {
                println!("user-test-heartbeat: control_pipe_closed");
                let _ = close(read_fd);
                exit(0);
            },
        }

        let now = now_us().unwrap_or(last_snapshot_us);
        if now >= next_print_us {
            heartbeat_seq += 1;
            println!(
                "user-test-heartbeat: seq={} now_us={} {} stale_for_ms={}",
                heartbeat_seq,
                now,
                snapshot,
                now.saturating_sub(last_snapshot_us) / 1000,
            );
            next_print_us = now.saturating_add(LTP_HEARTBEAT_PRINT_INTERVAL_US);
        }

        sleep_ltp_heartbeat_tick();
    }
}

fn drain_ltp_heartbeat_pipe(
    fd: Fd,
    snapshot: &mut String,
    pending: &mut String,
    last_snapshot_us: &mut i64,
) -> LtpHeartbeatPipe {
    let mut buf = [0u8; 256];
    loop {
        match read(fd, &mut buf) {
            Ok(0) => return LtpHeartbeatPipe::Closed,
            Ok(count) => {
                for &byte in &buf[..count] {
                    if byte == b'\n' {
                        snapshot.clear();
                        snapshot.push_str(pending.as_str());
                        pending.clear();
                        *last_snapshot_us = now_us().unwrap_or(*last_snapshot_us);
                    } else if byte.is_ascii_graphic() || byte == b' ' {
                        pending.push(byte as char);
                    } else {
                        pending.push('?');
                    }
                }
            },
            Err(EAGAIN) => return LtpHeartbeatPipe::Open,
            Err(EINTR) => {},
            Err(errno) => {
                println!("user-test-heartbeat: read failed: {errno:?}");
                return LtpHeartbeatPipe::Closed;
            },
        }
    }
}

fn sleep_ltp_heartbeat_tick() {
    let duration = TimeSpec {
        tv_sec: LTP_HEARTBEAT_SLEEP_TICK_SECONDS,
        tv_nsec: 0,
    };

    loop {
        match nanosleep(duration) {
            Ok(()) => return,
            Err(EINTR) => {},
            Err(errno) => {
                println!("user-test-heartbeat: nanosleep failed: {errno:?}");
                let _ = sched_yield();
                return;
            },
        }
    }
}

pub fn install_ltp_fixtures() {
    println!("user-test: installing LTP fixtures...");

    for fixture in LTP_FIXTURES {
        install_ltp_fixture(fixture);
        println!("user-test: ensured LTP fixture {}", fixture.path);
    }
}

pub fn run_ltp_tests() {
    let groups = select_ltp_groups();
    let mut heartbeat = LtpHeartbeat::start_or_disabled();

    print!("user-test: running LTP profile groups:");
    for group in &groups {
        print!(" {}", group.name);
    }
    println!();
    heartbeat.publish("profile_start", "-", "-", "-", 0);

    let mut overall = LtpSummary::default();
    for root in LTP_ROOTS {
        let summary = run_ltp_root(root, groups.as_slice(), &mut heartbeat);
        overall.merge(summary);
    }

    heartbeat.publish("profile_finished", "-", "-", "-", 0);
    println!(
        "user-test: LTP whitelist finished: attempted={} passed={} failed={} infra_failed={} skipped={}",
        overall.attempted, overall.passed, overall.failed, overall.infra_failed, overall.skipped,
    );
    heartbeat.finish();
}

fn run_ltp_root(
    root: &LtpRoot,
    groups: &[&'static LtpGroup],
    heartbeat: &mut LtpHeartbeat,
) -> LtpSummary {
    crate::switch_runtime(root.family);
    crate::clear_tmp();
    heartbeat.publish("root_start", root.label, "-", "-", 0);

    println!("#### OS COMP TEST GROUP START {} ####", root.label);
    let mut summary = LtpSummary::default();
    if fstatat(AtFd::Cwd, Path::new(root.workdir)).is_err() {
        println!(
            "user-test: skipping {} because {} is missing",
            root.label, root.workdir,
        );
        println!("#### OS COMP TEST GROUP END {} ####", root.label);
        heartbeat.publish("root_skipped", root.label, "-", "-", 0);
        return summary;
    }

    for group in groups {
        let group_summary = run_ltp_group(root, group, heartbeat);
        summary.merge(group_summary);
    }
    println!("#### OS COMP TEST GROUP END {} ####", root.label);

    heartbeat.publish("root_finished", root.label, "-", "-", 0);
    println!(
        "user-test: {} summary attempted={} passed={} failed={} infra_failed={} skipped={}",
        root.label,
        summary.attempted,
        summary.passed,
        summary.failed,
        summary.infra_failed,
        summary.skipped,
    );
    summary
}

fn run_ltp_group(root: &LtpRoot, group: &LtpGroup, heartbeat: &mut LtpHeartbeat) -> LtpSummary {
    // println!(
    //     "#### OS COMP TEST GROUP START {}/{} ####",
    //     root.label, group.name,
    // );
    heartbeat.publish("group_start", root.label, group.name, "-", 0);

    let mut summary = LtpSummary::default();
    for line in group.cases.lines() {
        let Some(case) = parse_case_line(line) else {
            continue;
        };

        if root
            .disabled_cases
            .iter()
            .any(|disabled| *disabled == case.name)
        {
            summary.skipped += 1;
            continue;
        }

        let case_path = format!("/{}/ltp/testcases/bin/{}", root.family, case.executable);
        if fstatat(AtFd::Cwd, Path::new(case_path.as_str())).is_err() {
            println!(
                "user-test: skipping {} missing case {} executable {}",
                root.label, case.name, case.executable,
            );
            summary.skipped += 1;
            continue;
        }

        summary.attempted += 1;
        match run_ltp_case(root, group, &case, case_path.as_str(), heartbeat) {
            LtpCaseOutcome::Passed => summary.passed += 1,
            LtpCaseOutcome::Failed => summary.failed += 1,
            LtpCaseOutcome::InfraFailed => summary.infra_failed += 1,
            LtpCaseOutcome::Skipped => summary.skipped += 1,
        }
    }

    // println!(
    //     "#### OS COMP TEST GROUP END {}/{} ####",
    //     root.label, group.name,
    // );
    heartbeat.publish("group_finished", root.label, group.name, "-", 0);
    println!(
        "user-test: {}/{} summary attempted={} passed={} failed={} infra_failed={} skipped={}",
        root.label,
        group.name,
        summary.attempted,
        summary.passed,
        summary.failed,
        summary.infra_failed,
        summary.skipped,
    );
    summary
}

fn select_ltp_groups() -> Vec<&'static LtpGroup> {
    let mut selected: Vec<&'static LtpGroup> = Vec::new();
    let mut select_all = false;

    for line in ACTIVE_PROFILE.lines() {
        let Some(name) = parse_profile_line(line) else {
            continue;
        };

        if name == "all" {
            if select_all || !selected.is_empty() {
                panic!("user-test: LTP profile uses all with other groups");
            }
            select_all = true;
            continue;
        }

        if select_all {
            panic!("user-test: LTP profile uses all with other groups");
        }

        let group = find_ltp_group(name)
            .unwrap_or_else(|| panic!("user-test: unknown LTP profile group {name}"));
        if selected.iter().any(|selected| selected.name == group.name) {
            panic!("user-test: duplicate LTP profile group {name}");
        }
        selected.push(group);
    }

    if select_all {
        selected.extend(LTP_GROUPS.iter());
    }

    if selected.is_empty() {
        panic!("user-test: LTP profile selected no groups");
    }

    selected
}

fn find_ltp_group(name: &str) -> Option<&'static LtpGroup> {
    LTP_GROUPS.iter().find(|group| group.name == name)
}

fn run_ltp_case(
    root: &LtpRoot,
    group: &LtpGroup,
    case: &LtpCaseSpec<'_>,
    case_path: &str,
    heartbeat: &mut LtpHeartbeat,
) -> LtpCaseOutcome {
    println!("\nRUN LTP CASE {}", case.name);
    heartbeat.publish("case_start", root.label, group.name, case.name, 0);

    match fork() {
        Ok(Some(tid)) => {
            heartbeat.publish("case_waiting", root.label, group.name, case.name, tid);
            match wait_ltp_case_status(tid, case.name, root.label, group.name, heartbeat) {
                Ok(LtpCaseWaitResult::Exited(wstatus)) => {
                    let exit_code = ltp_exit_code(wstatus);
                    if exit_code == 0 {
                        println!("FAIL LTP CASE {} : {}", case.name, exit_code);
                        heartbeat.publish("case_passed", root.label, group.name, case.name, tid);
                        LtpCaseOutcome::Passed
                    } else if exit_code == LTP_TCONF_EXIT_CODE {
                        println!("FAIL LTP CASE {} : {}", case.name, exit_code);
                        heartbeat.publish("case_skipped", root.label, group.name, case.name, tid);
                        LtpCaseOutcome::Skipped
                    } else {
                        println!("FAIL LTP CASE {} : {}", case.name, exit_code);
                        heartbeat.publish("case_failed", root.label, group.name, case.name, tid);
                        LtpCaseOutcome::Failed
                    }
                },
                Ok(LtpCaseWaitResult::TimedOut) => {
                    println!(
                        "FAIL LTP CASE {} : {}",
                        case.name, LTP_CASE_TIMEOUT_EXIT_CODE,
                    );
                    heartbeat.publish("case_timeout", root.label, group.name, case.name, tid);
                    LtpCaseOutcome::InfraFailed
                },
                Err(errno) => {
                    println!("user-test: {} wait failed: {errno:?}", case.name);
                    println!("FAIL LTP CASE {} : 127", case.name);
                    heartbeat.publish("case_wait_failed", root.label, group.name, case.name, tid);
                    LtpCaseOutcome::InfraFailed
                },
            }
        },
        Ok(None) => {
            if let Err(errno) = setpgid(0, 0) {
                println!(
                    "user-test: INFRA {} setpgid(0, 0) failed before execve: {errno:?}; not running case",
                    case.name,
                );
                exit(127);
            }

            if let Err(errno) = chdir(root.workdir) {
                println!(
                    "user-test: {} chdir({}) failed: {errno:?}",
                    case.name, root.workdir,
                );
                exit(127);
            }

            let mut argv = Vec::with_capacity(case.args.len() + 1);
            argv.push(case.executable);
            argv.extend(case.args.iter().copied());

            if let Err(errno) = execve(case_path, argv.as_slice(), root.envp) {
                println!(
                    "user-test: {} execve({}) failed: {errno:?}",
                    case.name, case_path,
                );
            }
            exit(127);
        },
        Err(errno) => {
            println!("user-test: {} fork failed: {errno:?}", case.name);
            println!("FAIL LTP CASE {} : 127", case.name);
            heartbeat.publish("case_fork_failed", root.label, group.name, case.name, 0);
            LtpCaseOutcome::InfraFailed
        },
    }
}

fn wait_ltp_case_status(
    tid: u32,
    name: &str,
    root_label: &str,
    group_name: &str,
    heartbeat: &mut LtpHeartbeat,
) -> Result<LtpCaseWaitResult, Errno> {
    let mut probe = LtpWaitLoopProbe::new(root_label, group_name, name, tid);
    probe.begin_iteration();
    let start_us = now_us_with_probe(&mut probe)?;
    probe.finish_iteration();
    loop {
        probe.begin_iteration();
        if let Some(wstatus) = poll_ltp_case_status(tid, name, &probe)? {
            return Ok(LtpCaseWaitResult::Exited(wstatus));
        }

        if elapsed_us_since_with_probe(start_us, &mut probe)? >= LTP_CASE_TIMEOUT_US {
            probe.finish_iteration();
            break;
        }

        sched_yield_with_probe(&probe)?;
        probe.finish_iteration();
    }

    println!(
        "user-test: TIMEOUT LTP CASE {name}: exceeded {LTP_CASE_TIMEOUT_SECONDS}s; killing case process group",
    );
    heartbeat.publish("case_timeout_kill", root_label, group_name, name, tid);
    kill_ltp_case(tid, name)?;

    probe.set_phase("kill_grace");
    let kill_start_us = now_us_with_probe(&mut probe)?;
    loop {
        probe.begin_iteration();
        if poll_ltp_case_status(tid, name, &probe)?.is_some() {
            return Ok(LtpCaseWaitResult::TimedOut);
        }

        if elapsed_us_since_with_probe(kill_start_us, &mut probe)? >= LTP_CASE_KILL_GRACE_US {
            println!(
                "user-test: TIMEOUT LTP CASE {name}: child not reaped after {LTP_CASE_KILL_GRACE_SECONDS}s kill grace; continuing",
            );
            heartbeat.publish("case_timeout_unreaped", root_label, group_name, name, tid);
            return Ok(LtpCaseWaitResult::TimedOut);
        }

        sched_yield_with_probe(&probe)?;
        probe.finish_iteration();
    }
}

fn poll_ltp_case_status(
    tid: u32,
    name: &str,
    probe: &LtpWaitLoopProbe<'_>,
) -> Result<Option<WStatus>, Errno> {
    let mut wstatus = WStatusRaw::EMPTY;
    probe.before("wait4");
    match wait4(
        WaitFor::ChildWithTgid(tid),
        Some(&mut wstatus),
        WaitOptions::NOHANG,
    ) {
        Ok(Some(waited)) => {
            probe.after("wait4", format_args!("waited={}", waited));
            if waited != tid {
                println!("user-test: {name} waited pid mismatch");
                return Err(ECHILD);
            }
            Ok(Some(wstatus.read()))
        },
        Ok(None) => {
            probe.after("wait4", format_args!("none"));
            Ok(None)
        },
        Err(EINTR) => {
            probe.after("wait4", format_args!("eintr"));
            Ok(None)
        },
        Err(errno) => {
            probe.after("wait4", format_args!("err={errno:?}"));
            Err(errno)
        },
    }
}

fn now_us_with_probe(probe: &mut LtpWaitLoopProbe<'_>) -> Result<i64, Errno> {
    probe.before("gettimeofday");
    match now_us() {
        Ok(now) => {
            probe.observe_now(now);
            probe.after("gettimeofday", format_args!("now_us={}", now));
            Ok(now)
        },
        Err(errno) => {
            probe.after("gettimeofday", format_args!("err={errno:?}"));
            Err(errno)
        },
    }
}

fn elapsed_us_since_with_probe(
    start_us: i64,
    probe: &mut LtpWaitLoopProbe<'_>,
) -> Result<i64, Errno> {
    Ok(now_us_with_probe(probe)?.saturating_sub(start_us))
}

fn sched_yield_with_probe(probe: &LtpWaitLoopProbe<'_>) -> Result<(), Errno> {
    probe.before("sched_yield");
    match sched_yield() {
        Ok(()) => {
            probe.after("sched_yield", format_args!("ok"));
            Ok(())
        },
        Err(errno) => {
            probe.after("sched_yield", format_args!("err={errno:?}"));
            Err(errno)
        },
    }
}

fn kill_ltp_case(tid: u32, name: &str) -> Result<(), Errno> {
    let pid = i32::try_from(tid).map_err(|_| EINVAL)?;

    // The child calls setpgid(0, 0) before exec, so -pid normally reaches the
    // whole case tree. The direct-pid fallback only covers the pre-setpgid
    // window and is not a substitute for process-group cleanup.
    match kill(-pid, SigNo::SIGKILL) {
        Ok(()) => Ok(()),
        Err(ESRCH) => {
            println!(
                "user-test: TIMEOUT LTP CASE {name}: process group -{tid} is absent; killing child pid {tid}",
            );
            match kill(pid, SigNo::SIGKILL) {
                Ok(()) | Err(ESRCH) => Ok(()),
                Err(errno) => Err(errno),
            }
        },
        Err(errno) => Err(errno),
    }
}

fn now_us() -> Result<i64, Errno> {
    let tv = gettimeofday()?;
    Ok(tv
        .tv_sec
        .saturating_mul(MICROS_PER_SECOND)
        .saturating_add(tv.tv_usec))
}

fn elapsed_us_since(start_us: i64) -> Result<i64, Errno> {
    Ok(now_us()?.saturating_sub(start_us))
}

fn parse_profile_line(line: &str) -> Option<&str> {
    let line = line.split('#').next().unwrap_or("").trim();
    if line.is_empty() {
        return None;
    }

    let mut parts = line.split_ascii_whitespace();
    let name = parts.next()?;
    if parts.next().is_some() {
        panic!("user-test: invalid LTP profile line {line}");
    }
    Some(name)
}

fn parse_case_line(line: &str) -> Option<LtpCaseSpec<'_>> {
    let line = line.split('#').next().unwrap_or("").trim();
    if line.is_empty() {
        return None;
    }

    if line.contains("=>") {
        panic!("user-test: invalid LTP case line {line}: use 'case [executable][: args...]'");
    }

    let (header, args) = match line.split_once(':') {
        Some((header, args)) => {
            let args = parse_case_args(args, line);
            if args.is_empty() {
                panic!("user-test: invalid LTP case line {line}: missing arguments");
            }
            (header, args)
        },
        None => (line, Vec::new()),
    };

    let mut header_parts = header.split_ascii_whitespace();
    let name = header_parts.next()?;
    let executable = header_parts.next().unwrap_or(name);
    if header_parts.next().is_some() {
        panic!("user-test: invalid LTP case line {line}: invalid case header");
    }
    Some(LtpCaseSpec {
        name,
        executable,
        args,
    })
}

fn parse_case_args<'a>(args: &'a str, line: &str) -> Vec<&'a str> {
    let mut parsed = Vec::new();
    let bytes = args.as_bytes();
    let mut idx = 0;

    while idx < bytes.len() {
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx == bytes.len() {
            break;
        }

        let start = idx;
        let token = match bytes[idx] {
            b'"' | b'\'' => {
                let quote = bytes[idx];
                idx += 1;
                let token_start = idx;
                while idx < bytes.len() && bytes[idx] != quote {
                    idx += 1;
                }
                if idx == bytes.len() {
                    panic!("user-test: invalid LTP case line {line}: unterminated quoted argument");
                }
                let token = &args[token_start..idx];
                idx += 1;
                if idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
                    panic!(
                        "user-test: invalid LTP case line {line}: quoted argument must end at token boundary",
                    );
                }
                token
            },
            b => {
                if b == b'\\' {
                    panic!(
                        "user-test: invalid LTP case line {line}: backslash escaping is unsupported",
                    );
                }
                idx += 1;
                while idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
                    if matches!(bytes[idx], b'"' | b'\'' | b'\\') {
                        panic!(
                            "user-test: invalid LTP case line {line}: quotes are only supported around a whole argument",
                        );
                    }
                    idx += 1;
                }
                &args[start..idx]
            },
        };
        parsed.push(token);
    }

    parsed
}

fn ltp_exit_code(wstatus: WStatus) -> i32 {
    match wstatus {
        WStatus::Exited(code) => i32::from(code),
        WStatus::Signal(sig) | WStatus::Stopped(sig) => 128 + i32::from(sig),
        WStatus::Continued => 128,
    }
}

fn install_ltp_fixture(fixture: &LtpFixture) {
    let parent = fixture.path.rsplit_once('/').map(|(parent, _)| parent);
    let parent = parent.filter(|parent| !parent.is_empty()).unwrap_or("/");
    let script = format!(
        "mkdir -p {parent} && cat > {path} <<'EOF'\n{content}\nEOF",
        path = fixture.path,
        parent = parent,
        content = fixture.content,
    );

    crate::run_busybox(
        &["busybox", "sh", "-c", script.as_str()],
        "install LTP fixture",
    );
}
