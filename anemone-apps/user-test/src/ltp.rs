use anemone_rs::{
    os::linux::{
        fs::{chdir, fstatat, AtFd},
        process::{execve, exit, fork, setpgid, WStatus},
    },
    prelude::*,
};

const ACTIVE_PROFILE: &str = include_str!("../ltp/profile.txt");
const ETC_PASSWD: &str = include_str!("../fixtures/passwd");
const ETC_GROUP: &str = include_str!("../fixtures/group");
const LTP_KCONFIG: &str = include_str!("../fixtures/ltp-kconfig");
const LTP_MODULES_BUILTIN: &str = include_str!("../fixtures/modules.builtin");
const LTP_MODULES_DEP: &str = include_str!("../fixtures/modules.dep");

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
        name: "full",
        cases: include_str!("../ltp/groups/full.txt"),
    },
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
        name: "schedule",
        cases: include_str!("../ltp/groups/schedule.txt"),
    },
    LtpGroup {
        name: "iomux",
        cases: include_str!("../ltp/groups/iomux.txt"),
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

    print!("user-test: running LTP profile groups:");
    for group in &groups {
        print!(" {}", group.name);
    }
    println!();

    let mut overall = LtpSummary::default();
    for root in LTP_ROOTS {
        let summary = run_ltp_root(root, groups.as_slice());
        overall.merge(summary);
    }

    println!(
        "user-test: LTP whitelist finished: attempted={} passed={} failed={} infra_failed={} skipped={}",
        overall.attempted, overall.passed, overall.failed, overall.infra_failed, overall.skipped,
    );
}

fn run_ltp_root(root: &LtpRoot, groups: &[&'static LtpGroup]) -> LtpSummary {
    crate::switch_runtime(root.family);
    crate::clear_tmp();

    let mut summary = LtpSummary::default();
    if fstatat(AtFd::Cwd, Path::new(root.workdir)).is_err() {
        println!("#### OS COMP TEST GROUP START {} ####", root.label);
        println!(
            "user-test: skipping {} because {} is missing",
            root.label, root.workdir,
        );
        println!("#### OS COMP TEST GROUP END {} ####", root.label);
        return summary;
    }

    for group in groups {
        let group_summary = run_ltp_group(root, group);
        summary.merge(group_summary);
    }

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

fn run_ltp_group(root: &LtpRoot, group: &LtpGroup) -> LtpSummary {
    println!(
        "#### OS COMP TEST GROUP START {}/{} ####",
        root.label, group.name,
    );

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
        match run_ltp_case(root, &case, case_path.as_str()) {
            LtpCaseOutcome::Passed => summary.passed += 1,
            LtpCaseOutcome::Failed => summary.failed += 1,
            LtpCaseOutcome::InfraFailed => summary.infra_failed += 1,
        }
    }

    println!(
        "#### OS COMP TEST GROUP END {}/{} ####",
        root.label, group.name,
    );
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

fn run_ltp_case(root: &LtpRoot, case: &LtpCaseSpec<'_>, case_path: &str) -> LtpCaseOutcome {
    println!("\nRUN LTP CASE {}", case.name);

    match fork() {
        Ok(Some(tid)) => match crate::wait_child_status(tid, case.name) {
            Ok(wstatus) => {
                let exit_code = ltp_exit_code(wstatus);
                if exit_code == 0 {
                    println!("PASS LTP CASE {} : {}", case.name, exit_code);
                    LtpCaseOutcome::Passed
                } else {
                    println!("FAIL LTP CASE {} : {}", case.name, exit_code);
                    LtpCaseOutcome::Failed
                }
            },
            Err(errno) => {
                println!("user-test: {} wait failed: {errno:?}", case.name);
                println!("FAIL LTP CASE {} : 127", case.name);
                LtpCaseOutcome::InfraFailed
            },
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
            LtpCaseOutcome::InfraFailed
        },
    }
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
            let args = args.split_ascii_whitespace().collect::<Vec<_>>();
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
